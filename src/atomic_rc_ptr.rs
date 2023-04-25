use std::{marker::PhantomData, mem};

use atomic::{Atomic, Ordering};
use static_assertions::const_assert;

use crate::{
    internal::{AcquireRetire, AcquiredPtr, MarkedCntObjPtr, MarkedPtr},
    rc_ptr::RcPtr,
    snapshot_ptr::SnapshotPtr,
    LocalPtr,
};

pub struct AtomicRcPtr<T, Guard>
where
    Guard: AcquireRetire,
{
    link: Atomic<MarkedCntObjPtr<T>>,
    _marker: PhantomData<Guard>,
}

unsafe impl<T, Guard: AcquireRetire> Send for AtomicRcPtr<T, Guard> {}
unsafe impl<T, Guard: AcquireRetire> Sync for AtomicRcPtr<T, Guard> {}

// Ensure that MarkedPtr<T> is 8-byte long,
// so that lock-free atomic operations are possible.
const_assert!(Atomic::<MarkedCntObjPtr<u8>>::is_lock_free());
const_assert!(mem::size_of::<MarkedCntObjPtr<u8>>() == mem::size_of::<*mut u8>());

impl<T, Guard> AtomicRcPtr<T, Guard>
where
    Guard: AcquireRetire,
{
    #[inline(always)]
    pub fn new(obj: T, guard: &Guard) -> Self {
        Self::from_rc(RcPtr::make_shared(obj, guard))
    }

    #[inline(always)]
    pub fn from_rc(ptr: RcPtr<T, Guard>) -> Self {
        Self {
            link: Atomic::new(ptr.release()),
            _marker: PhantomData,
        }
    }

    #[inline(always)]
    pub fn from_snapshot<'g>(ptr: SnapshotPtr<'g, T, Guard>, guard: &'g Guard) -> Self {
        let result = Self::null();
        result.store_snapshot(ptr, Ordering::Relaxed, guard);
        result
    }

    #[inline(always)]
    pub fn from_local<'g, P>(ptr: &P, guard: &'g Guard) -> Self
    where
        P: LocalPtr<'g, T, Guard>,
    {
        let ptr = ptr.as_counted_ptr();
        if !ptr.is_null() {
            unsafe { guard.increment_ref_cnt(ptr.unmarked()) };
        }
        Self {
            link: Atomic::new(ptr),
            _marker: PhantomData,
        }
    }

    #[inline(always)]
    pub fn null() -> Self {
        Self {
            link: Atomic::new(MarkedPtr::null()),
            _marker: PhantomData,
        }
    }

    #[inline(always)]
    pub fn store_null(&self, guard: &Guard) {
        let old = self.link.swap(MarkedPtr::null(), Ordering::SeqCst);
        if !old.is_null() {
            unsafe { guard.delayed_decrement_ref_cnt(old.unmarked()) };
        }
    }

    #[inline(always)]
    pub fn store(&self, desired: RcPtr<T, Guard>, order: Ordering, guard: &Guard) {
        let new_ptr = desired.release();
        let old_ptr = self.link.swap(new_ptr, order);
        if !old_ptr.is_null() {
            unsafe { guard.delayed_decrement_ref_cnt(old_ptr.unmarked()) }
        }
    }

    /// A variation of `store_rc` which use relaxed load/store instead of swap
    #[inline(always)]
    pub fn store_relaxed(&self, desired: RcPtr<T, Guard>, guard: &Guard) {
        let new_ptr = desired.release();
        let old_ptr = self.link.load(Ordering::Relaxed);
        self.link.store(new_ptr, Ordering::Release);
        if !old_ptr.is_null() {
            unsafe { guard.delayed_decrement_ref_cnt(old_ptr.unmarked()) }
        }
    }

    #[inline(always)]
    pub fn store_snapshot(&self, desired: SnapshotPtr<T, Guard>, order: Ordering, guard: &Guard) {
        // For converting a SnapshotPtr into an RcPtr,
        // as the ref count has already been incremented,
        // the pointer can just be transferred.
        let new_ptr = desired.as_counted_ptr();

        // If desired is protected, a small optimization opportunity is to not
        // increment/decrement the reference count of the new/old value if they
        // turn out to be the same. If desired isn't protected, we must proactively
        // increment, though, otherwise it could be decremented after we exchange
        // but before we perform the increment.
        unsafe {
            if desired.is_protected() {
                let old_ptr = self.link.swap(new_ptr, order);
                if old_ptr != new_ptr {
                    if !new_ptr.is_null() {
                        guard.increment_ref_cnt(new_ptr.unmarked());
                    }
                    if !old_ptr.is_null() {
                        guard.delayed_decrement_ref_cnt(old_ptr.unmarked());
                    }
                }
            } else {
                if !new_ptr.is_null() {
                    guard.increment_ref_cnt(new_ptr.unmarked());
                }
                let old_ptr = self.link.swap(new_ptr, order);
                if !old_ptr.is_null() {
                    guard.delayed_decrement_ref_cnt(old_ptr.unmarked());
                }
            }
        }
    }

    #[inline(always)]
    pub fn load<'g>(&self, guard: &'g Guard) -> RcPtr<'g, T, Guard> {
        let acquired = guard.acquire(&self.link);
        RcPtr::new_with_incr(acquired.as_counted_ptr(), guard)
    }

    #[inline(always)]
    pub fn load_snapshot<'g>(&self, guard: &'g Guard) -> SnapshotPtr<'g, T, Guard> {
        SnapshotPtr::new(guard.protect_snapshot(&self.link), guard)
    }

    /// Swap the currently stored shared pointer with the given shared pointer.
    /// This operation is thread-safe.
    /// (It is equivalent to `exchange` from the original implementation.)
    #[inline(always)]
    pub fn swap<'g>(&self, desired: RcPtr<T, Guard>, guard: &'g Guard) -> RcPtr<'g, T, Guard> {
        let new_ptr = desired.release();
        RcPtr::new_without_incr(self.link.swap(new_ptr, Ordering::SeqCst), guard)
    }

    /// Atomically compares the underlying pointer with expected, and if they refer to
    /// the same managed object, replaces the current pointer with a copy of desired
    /// (incrementing its reference count) and returns true. Otherwise, returns false.
    #[inline(always)]
    pub fn compare_exchange<'g, P1, P2>(
        &self,
        expected: &P1,
        desired: &P2,
        guard: &'g Guard,
    ) -> Result<(), SnapshotPtr<'g, T, Guard>>
    where
        P1: LocalPtr<'g, T, Guard>,
        P2: LocalPtr<'g, T, Guard>,
    {
        // We need to make a reservation if the desired snapshot pointer no longer has
        // an announcement slot. Otherwise, desired is protected, assuming that another
        // thread can not clear the announcement slot (this might change one day!)
        let _reservation = if desired.is_protected() {
            guard.reserve_nothing()
        } else {
            guard.reserve(desired.as_counted_ptr().unmarked())
        };

        let desired_ptr = desired.as_counted_ptr();
        match self.compare_exchange_impl(expected.as_counted_ptr(), desired_ptr, guard) {
            Ok(()) => {
                if !desired_ptr.is_null() {
                    unsafe { guard.increment_ref_cnt(desired_ptr.unmarked()) };
                }
                Ok(())
            }
            Err(current) => Err(current),
        }
    }

    #[inline(always)]
    fn compare_exchange_impl<'g>(
        &self,
        expected: MarkedCntObjPtr<T>,
        desired: MarkedCntObjPtr<T>,
        guard: &'g Guard,
    ) -> Result<(), SnapshotPtr<'g, T, Guard>> {
        loop {
            match self
                .link
                .compare_exchange(expected, desired, Ordering::SeqCst, Ordering::SeqCst)
            {
                Ok(_) => {
                    if !expected.is_null() {
                        unsafe { guard.delayed_decrement_ref_cnt(expected.unmarked()) };
                    }
                    return Ok(());
                }
                Err(_) => {
                    let loaded = self.load_snapshot(guard);
                    if loaded.as_usize() != expected.as_usize() {
                        return Err(loaded);
                    }
                }
            }
        }
    }

    #[inline(always)]
    pub fn compare_exchange_mark<'g, P>(
        &self,
        expected: &P,
        mark: usize,
        guard: &'g Guard,
    ) -> Result<(), SnapshotPtr<'g, T, Guard>>
    where
        P: LocalPtr<'g, T, Guard>,
    {
        let expected_ptr = expected.as_counted_ptr();
        let desired_ptr = expected_ptr.with_mark(mark);
        loop {
            match self
                .link
                .compare_exchange(
                    expected_ptr,
                    desired_ptr,
                    Ordering::SeqCst,
                    Ordering::SeqCst
                )
            {
                Ok(_) => return Ok(()),
                Err(_) => {
                    let loaded = self.load_snapshot(guard);
                    if loaded.as_usize() != expected.as_usize() {
                        return Err(loaded);
                    }
                }
            }
        }
    }

    #[inline(always)]
    pub fn fetch_or<'g>(&self, mark: usize, guard: &'g Guard) -> SnapshotPtr<'g, T, Guard> {
        let mut cur = self.load_snapshot(guard);
        let mut new = cur.as_counted_ptr().with_mark(cur.mark() | mark);
        while self
            .link
            .compare_exchange(
                cur.as_counted_ptr(),
                new,
                Ordering::SeqCst,
                Ordering::SeqCst,
            )
            .is_err()
        {
            cur = self.load_snapshot(guard);
            new = cur.as_counted_ptr().with_mark(cur.mark() | mark);
        }
        cur
    }
}

impl<T, Guard> Drop for AtomicRcPtr<T, Guard>
where
    Guard: AcquireRetire,
{
    #[inline(always)]
    fn drop(&mut self) {
        let ptr = self.link.load(Ordering::SeqCst);
        unsafe {
            if !ptr.is_null() {
                let guard = Guard::unprotected();
                guard.delayed_decrement_ref_cnt(ptr.unmarked());
            }
        }
    }
}

impl<T, Guard> Default for AtomicRcPtr<T, Guard>
where
    Guard: AcquireRetire,
{
    #[inline(always)]
    fn default() -> Self {
        Self::null()
    }
}
