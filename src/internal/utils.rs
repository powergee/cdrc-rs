use atomic::{Atomic, Ordering};
use core::mem;
use static_assertions::const_assert;
use std::{
    mem::ManuallyDrop,
    ptr,
    sync::atomic::{compiler_fence, AtomicBool},
};

pub(crate) type Count = u32;

/// A wait-free atomic counter that supports increment and decrement,
/// such that attempting to increment the counter from zero fails and
/// does not perform the increment.
///
/// Useful for implementing reference counting, where the underlying
/// managed memory is freed when the counter hits zero, so that other
/// racing threads can not increment the counter back up from zero
///
/// Assumption: The counter should never go negative. That is, the
/// user should never decrement the counter by an amount greater
/// than its current value
///
/// Note: The counter steals the top two bits of the integer for book-
/// keeping purposes. Hence the maximum representable value in the
/// counter is 2^(8*32-2) - 1
pub(crate) struct StickyCounter {
    x: Atomic<Count>,
}

const_assert!(Atomic::<Count>::is_lock_free());

impl StickyCounter {
    const fn zero_flag() -> Count {
        1 << (mem::size_of::<Count>() * 8 - 1)
    }

    const fn zero_pending_flag() -> Count {
        1 << (mem::size_of::<Count>() * 8 - 2)
    }

    pub fn new() -> Self {
        Self { x: Atomic::new(1) }
    }

    /// Increment the counter by the given amount if the counter is not zero.
    ///
    /// Returns true if the increment was successful, i.e., the counter
    /// was not stuck at zero. Returns false if the counter was zero
    pub fn increment(&self, add: Count, order: Ordering) -> bool {
        let val = self.x.fetch_add(add, order);
        (val & Self::zero_flag()) == 0
    }

    /// Decrement the counter by the given amount. The counter must initially be
    /// at least this amount, i.e., it is not permitted to decrement the counter
    /// to a negative number.
    ///
    /// Returns true if the counter was decremented to zero. Returns
    /// false if the counter was not decremented to zero
    pub fn decrement(&self, sub: Count, order: Ordering) -> bool {
        if self.x.fetch_sub(sub, order) == sub {
            match self
                .x
                .compare_exchange(0, Self::zero_flag(), Ordering::SeqCst, Ordering::SeqCst)
            {
                Ok(_) => return true,
                Err(actual) => {
                    return ((actual & Self::zero_pending_flag()) > 0)
                        && ((self.x.swap(Self::zero_flag(), Ordering::SeqCst)
                            & Self::zero_pending_flag())
                            > 0)
                }
            }
        }
        false
    }

    /// Loads the current value of the counter. If the current value is zero, it is guaranteed
    /// to remain zero until the counter is reset
    pub fn load(&self, order: Ordering) -> Count {
        let val = self.x.load(order);
        if val != 0 {
            return if (val & Self::zero_flag()) > 0 {
                0
            } else {
                val
            };
        }

        match self.x.compare_exchange(
            val,
            Self::zero_flag() | Self::zero_pending_flag(),
            Ordering::SeqCst,
            Ordering::SeqCst,
        ) {
            Ok(_) => 0,
            Err(actual) => {
                if (actual & Self::zero_flag()) > 0 {
                    0
                } else {
                    actual
                }
            }
        }
    }
}

impl From<Count> for StickyCounter {
    fn from(value: Count) -> Self {
        Self {
            x: Atomic::new(if value == 0 { Self::zero_flag() } else { value }),
        }
    }
}

pub enum EjectAction {
    Nothing,
    Delay,
    Destroy,
}

/// An instance of an object of type T with an atomic reference count.
pub struct CountedObject<T> {
    storage: ManuallyDrop<T>,
    ref_cnt: StickyCounter,
    weak_cnt: StickyCounter,
    disposed: AtomicBool,
}

impl<T> CountedObject<T> {
    pub fn new(val: T) -> Self {
        Self {
            storage: ManuallyDrop::new(val),
            ref_cnt: StickyCounter::new(),
            weak_cnt: StickyCounter::new(),
            disposed: AtomicBool::new(false),
        }
    }

    pub fn data(&self) -> &T {
        &self.storage
    }

    pub fn data_mut(&mut self) -> &mut T {
        &mut self.storage
    }

    /// Destroy the managed object, but keep the control data intact
    pub unsafe fn dispose(&mut self) {
        self.disposed.store(true, Ordering::Release);
        ManuallyDrop::drop(&mut self.storage)
    }

    pub fn use_count(&self) -> Count {
        self.ref_cnt.load(Ordering::SeqCst)
    }

    pub fn weak_count(&self) -> Count {
        self.weak_cnt.load(Ordering::SeqCst)
    }

    pub fn add_refs(&self, count: Count) -> bool {
        self.ref_cnt.increment(count, Ordering::SeqCst)
    }

    /// Release strong references to the object. If the strong reference count reaches zero,
    /// the managed object will be destroyed, and the weak reference count will be decremented
    /// by one. If this causes the weak reference count to hit zero, returns true, indicating
    /// that the caller should delete this object.
    pub fn release_refs(&mut self, count: Count) -> EjectAction {
        // A decrement-release + an acquire fence is recommended by Boost's documentation:
        // https://www.boost.org/doc/libs/1_57_0/doc/html/atomic/usage_examples.html
        // Alternatively, an acquire-release decrement would work, but might be less efficient since the
        // acquire is only relevant if the decrement zeros the counter.
        if self.ref_cnt.decrement(count, Ordering::Release) {
            compiler_fence(Ordering::Acquire);
            // If there are no live weak pointers, we can immediately destroy
            // everything. Otherwise, we have to defer the disposal of the
            // managed object since an atomic_weak_ptr might be about to
            // take a snapshot...
            if self.weak_cnt.load(Ordering::Relaxed) == 1 {
                // Immediately destroy the managed object and
                // collect the control data, since no more
                // live (strong or weak) references exist
                unsafe { self.dispose() };
                EjectAction::Destroy
            } else {
                // At least one weak reference exists, so we have to
                // delay the destruction of the managed object
                EjectAction::Delay
            }
        } else {
            EjectAction::Nothing
        }
    }

    pub fn add_weak_refs(&self, count: Count) -> bool {
        self.weak_cnt.increment(count, Ordering::Relaxed)
    }

    // Release weak references to the object. If this causes the weak reference count
    // to hit zero, returns true, indicating that the caller should delete this object.
    pub fn release_weak_refs(&self, count: Count) -> bool {
        self.weak_cnt.decrement(count, Ordering::Release)
    }
}

impl<T> Drop for CountedObject<T> {
    fn drop(&mut self) {
        assert!(self.disposed.load(Ordering::Acquire));
    }
}

pub struct MarkedPtr<T> {
    ptr: *mut T,
}

impl<T> Default for MarkedPtr<T> {
    fn default() -> Self {
        Self {
            ptr: ptr::null_mut(),
        }
    }
}

impl<T> Clone for MarkedPtr<T> {
    fn clone(&self) -> Self {
        Self { ptr: self.ptr }
    }
}

impl<T> Copy for MarkedPtr<T> {}

impl<T> PartialEq for MarkedPtr<T> {
    fn eq(&self, other: &Self) -> bool {
        self.ptr == other.ptr
    }
}

impl<T> MarkedPtr<T> {
    pub fn new(ptr: *mut T) -> Self {
        Self { ptr }
    }

    pub fn null() -> Self {
        Self {
            ptr: ptr::null_mut(),
        }
    }

    pub fn is_null(&self) -> bool {
        self.unmarked().is_null()
    }

    pub fn mark(&self) -> usize {
        let ptr = self.ptr as usize;
        ptr & low_bits::<T>()
    }

    pub fn unmarked(&self) -> *mut T {
        let ptr = self.ptr as usize;
        (ptr & !low_bits::<T>()) as *mut T
    }

    pub fn with_mark(&self, mark: usize) -> Self {
        Self::new(with_mark(self.ptr, mark))
    }

    pub fn set_ptr(&mut self, ptr: *mut T) {
        self.ptr = with_mark(ptr, self.mark());
    }

    pub fn set_mark(&mut self, mark: usize) {
        self.ptr = with_mark(self.ptr, mark);
    }

    pub unsafe fn deref<'g>(&self) -> &'g T {
        &*self.unmarked()
    }

    pub unsafe fn deref_mut<'g>(&mut self) -> &'g mut T {
        &mut *self.unmarked()
    }

    pub fn as_usize(&self) -> usize {
        self.ptr as usize
    }
}

/// Returns a bitmask containing the unused least significant bits of an aligned pointer to `T`.
#[inline]
const fn low_bits<T>() -> usize {
    (1 << mem::align_of::<T>().trailing_zeros()) - 1
}

/// Returns the pointer with the given mark
#[inline]
fn with_mark<T>(ptr: *mut T, mark: usize) -> *mut T {
    ((ptr as usize & !low_bits::<T>()) | (mark & low_bits::<T>())) as *mut T
}

pub(crate) type MarkedCntObjPtr<T> = MarkedPtr<CountedObject<T>>;
