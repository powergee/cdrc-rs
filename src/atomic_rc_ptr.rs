use std::{marker::PhantomData};

use atomic::{Atomic, Ordering};
use static_assertions::const_assert;

use crate::{internal::{AcquireRetire, CountedObjPtr, MarkedPtr}, rc_ptr::RcPtr};

pub struct AtomicRcPtr<T, S>
where
    S: AcquireRetire<T>,
{
    ptr: Atomic<CountedObjPtr<T>>,
    _marker: PhantomData<S>,
}

// Ensure that MarkedPtr<T> is 8-byte long,
// so that lock-free atomic operations are possible.
const_assert!(Atomic::<CountedObjPtr<u8>>::is_lock_free());

impl<T, S> AtomicRcPtr<T, S>
where
    S: AcquireRetire<T>,
{
    pub fn new() -> Self {
        Self {
            ptr: Atomic::new(MarkedPtr::null()),
            _marker: PhantomData,
        }
    }

    pub fn store_null(&self, guard: &S) {
        let old = self
            .ptr
            .swap(MarkedPtr::null(), Ordering::SeqCst);
        if !old.is_null() {
            unsafe { guard.delayed_decrement_ref_cnt(old.unmarked()) };
        }
    }

    pub fn store_rc_relaxed(&self, desired: RcPtr<T, S>) {
        
    }

    pub fn store_rc(&self, desired: RcPtr<T, S>) {
        
    }
}

impl<T, S> Drop for AtomicRcPtr<T, S>
where
    S: AcquireRetire<T>,
{
    fn drop(&mut self) {
        let ptr = self.ptr.load(Ordering::SeqCst);
        if !ptr.is_null() {
            unsafe { S::handle().delayed_decrement_ref_cnt(ptr.unmarked()) };
        }
    }
}
