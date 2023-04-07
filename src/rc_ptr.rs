use std::marker::PhantomData;

use crate::{
    internal::{AcquireRetire, Count, CountedObjPtr, MarkedPtr},
    snapshot_ptr::SnapshotPtr,
};

pub struct RcPtr<T, Guard>
where
    Guard: AcquireRetire<T>,
{
    ptr: CountedObjPtr<T>,
    _marker: PhantomData<Guard>,
}

impl<T, Guard> RcPtr<T, Guard>
where
    Guard: AcquireRetire<T>,
{
    pub(crate) fn new_with_incr(ptr: CountedObjPtr<T>, guard: &Guard) -> Self {
        let rc = Self {
            ptr,
            _marker: PhantomData,
        };
        unsafe { guard.increment_ref_cnt(rc.ptr.unmarked()) };
        rc
    }

    pub(crate) fn new_without_incr(ptr: CountedObjPtr<T>) -> Self {
        Self {
            ptr,
            _marker: PhantomData,
        }
    }

    pub fn from(ptr: &SnapshotPtr<T, Guard>, guard: &Guard) -> Self {
        let rc = Self {
            ptr: ptr.as_counted_ptr(),
            _marker: PhantomData,
        };
        unsafe { guard.increment_ref_cnt(rc.ptr.unmarked()) };
        rc
    }

    pub fn make_shared(obj: T, guard: &Guard) -> Self {
        let ptr = guard.create_object(obj);
        Self {
            ptr: CountedObjPtr::new(ptr),
            _marker: PhantomData,
        }
    }

    pub fn clone(&self, guard: &Guard) -> Self {
        let rc = Self {
            ptr: self.ptr,
            _marker: PhantomData,
        };
        unsafe { guard.increment_ref_cnt(rc.ptr.unmarked()) };
        rc
    }

    pub fn clear(&mut self, guard: &Guard) {
        if !self.ptr.is_null() {
            unsafe { guard.decrement_ref_cnt(self.ptr.unmarked()) };
        }
        self.ptr = MarkedPtr::null();
    }

    pub fn is_null(&self) -> bool {
        self.ptr.is_null()
    }

    pub unsafe fn deref(&self) -> &T {
        self.ptr.deref().data()
    }

    pub unsafe fn deref_mut(&self) -> &mut T {
        self.ptr.deref_mut().data_mut()
    }

    pub fn use_count(&self) -> Count {
        unsafe { self.ptr.deref().use_count() }
    }

    pub fn weak_count(&self) -> Count {
        unsafe { self.ptr.deref().weak_count() }
    }

    pub fn release(self) -> CountedObjPtr<T> {
        self.ptr
    }

    pub fn as_counted_ptr(&self) -> CountedObjPtr<T> {
        self.ptr
    }
}

impl<T, Guard> Drop for RcPtr<T, Guard>
where
    Guard: AcquireRetire<T>,
{
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            self.clear(&Guard::handle());
        }
    }
}
