use std::{marker::PhantomData, mem};

use atomic::{Atomic, Ordering};
use static_assertions::const_assert;

use crate::{
    internal::{AcquireRetire, AcquiredPtr, MarkedCntObjPtr, MarkedPtr},
    rc_ptr::RcPtr,
    snapshot_ptr::SnapshotPtr,
};

pub struct AtomicRcPtr<T, Guard>
where
    Guard: AcquireRetire,
{
    link: Atomic<MarkedCntObjPtr<T>>,
    _marker: PhantomData<Guard>,
}

// Ensure that MarkedPtr<T> is 8-byte long,
// so that lock-free atomic operations are possible.
const_assert!(Atomic::<MarkedCntObjPtr<u8>>::is_lock_free());
const_assert!(mem::size_of::<MarkedCntObjPtr<u8>>() == mem::size_of::<*mut u8>());

impl<T, Guard> AtomicRcPtr<T, Guard>
where
    Guard: AcquireRetire,
{
    pub fn null() -> Self {
        Self {
            link: Atomic::new(MarkedPtr::null()),
            _marker: PhantomData,
        }
    }

    pub fn store_null(&self, guard: &Guard) {
        let old = self.link.swap(MarkedPtr::null(), Ordering::SeqCst);
        if !old.is_null() {
            unsafe { guard.delayed_decrement_ref_cnt(old.unmarked()) };
        }
    }

    pub fn store_rc(&self, desired: RcPtr<T, Guard>, order: Ordering, guard: &Guard) {
        let new_ptr = desired.release();
        let old_ptr = self.link.swap(new_ptr, order);
        if !old_ptr.is_null() {
            unsafe { guard.delayed_decrement_ref_cnt(old_ptr.unmarked()) }
        }
    }

    /// A variation of `store_rc` which use relaxed load/store instead of swap
    pub fn store_rc_relaxed(&self, desired: RcPtr<T, Guard>, guard: &Guard) {
        let new_ptr = desired.release();
        let old_ptr = self.link.load(Ordering::Relaxed);
        self.link.store(new_ptr, Ordering::Release);
        if !old_ptr.is_null() {
            unsafe { guard.delayed_decrement_ref_cnt(old_ptr.unmarked()) }
        }
    }

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

    pub fn load_rc(&self, guard: &Guard) -> RcPtr<T, Guard> {
        let acquired = guard.acquire(&self.link);
        RcPtr::new_with_incr(acquired.as_counted_ptr(), guard)
    }

    pub fn load_snapshot(&self, guard: &Guard) -> SnapshotPtr<T, Guard> {
        SnapshotPtr::new(guard.protect_snapshot(&self.link))
    }

    /// Swap the currently stored shared pointer with the given shared pointer.
    /// This operation is thread-safe.
    /// (It is equivalent to `exchange` from the original implementation.)
    pub fn swap(&self, desired: RcPtr<T, Guard>) -> RcPtr<T, Guard> {
        let new_ptr = desired.release();
        RcPtr::new_without_incr(self.link.swap(new_ptr, Ordering::SeqCst))
    }

    pub fn compare_exchange(
        &self,
        expected: RcPtr<T, Guard>,
        desired: RcPtr<T, Guard>,
        guard: &Guard,
    ) -> bool {
        self.compare_exchange_inner(expected.as_counted_ptr(), desired.release(), guard)
    }

    fn compare_exchange_inner(
        &self,
        expected: MarkedCntObjPtr<T>,
        desired: MarkedCntObjPtr<T>,
        guard: &Guard,
    ) -> bool {
        if self
            .link
            .compare_exchange(expected, desired, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            if !expected.is_null() {
                unsafe { guard.delayed_decrement_ref_cnt(expected.unmarked()) };
            }
            return true;
        }
        false
    }
}

impl<T, Guard> Drop for AtomicRcPtr<T, Guard>
where
    Guard: AcquireRetire,
{
    fn drop(&mut self) {
        let ptr = self.link.load(Ordering::SeqCst);
        if !ptr.is_null() {
            unsafe { Guard::handle().delayed_decrement_ref_cnt(ptr.unmarked()) };
        }
    }
}
