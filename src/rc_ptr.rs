use std::marker::PhantomData;

use crate::{
    internal::{AcquireRetire, Count, MarkedCntObjPtr, MarkedPtr},
    snapshot_ptr::SnapshotPtr,
};

pub struct RcPtr<T, Guard>
where
    Guard: AcquireRetire,
{
    ptr: MarkedCntObjPtr<T>,
    _marker: PhantomData<Guard>,
}

impl<T, Guard> RcPtr<T, Guard>
where
    Guard: AcquireRetire,
{
    pub(crate) fn new_with_incr(ptr: MarkedCntObjPtr<T>, guard: &Guard) -> Self {
        let rc = Self {
            ptr,
            _marker: PhantomData,
        };
        unsafe { guard.increment_ref_cnt(rc.ptr.unmarked()) };
        rc
    }

    pub(crate) fn new_without_incr(ptr: MarkedCntObjPtr<T>) -> Self {
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
            ptr: MarkedCntObjPtr::new(ptr),
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

    pub fn as_ref(&self) -> Option<&T> {
        if self.is_null() {
            None
        } else {
            Some(unsafe { self.deref() })
        }
    }

    /// # Safety
    /// TODO
    pub unsafe fn deref(&self) -> &T {
        self.ptr.deref().data()
    }

    /// # Safety
    /// TODO
    pub unsafe fn deref_mut(&mut self) -> &mut T {
        self.ptr.deref_mut().data_mut()
    }

    pub fn use_count(&self) -> Count {
        unsafe { self.ptr.deref().use_count() }
    }

    pub fn weak_count(&self) -> Count {
        unsafe { self.ptr.deref().weak_count() }
    }

    pub fn release(self) -> MarkedCntObjPtr<T> {
        self.ptr
    }

    pub(crate) fn as_counted_ptr(&self) -> MarkedCntObjPtr<T> {
        self.ptr
    }

    pub fn mark(&self) -> usize {
        self.ptr.mark()
    }

    pub fn unmarked(self) -> Self {
        Self::new_without_incr(MarkedCntObjPtr::new(self.ptr.unmarked()))
    }

    pub fn with_mark(self, mark: usize) -> Self {
        Self::new_without_incr(self.ptr.marked(mark))
    }

    pub fn eq_without_tag(&self, rhs: &Self) -> bool {
        self.as_counted_ptr().unmarked() == rhs.as_counted_ptr().unmarked()
    }
}

impl<T, Guard> Drop for RcPtr<T, Guard>
where
    Guard: AcquireRetire,
{
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            self.clear(&Guard::handle());
        }
    }
}

impl<T, Guard> PartialEq for RcPtr<T, Guard>
where
    Guard: AcquireRetire,
{
    fn eq(&self, other: &Self) -> bool {
        self.ptr == other.ptr
    }
}

impl<T, Guard> Default for RcPtr<T, Guard>
where
    Guard: AcquireRetire,
{
    fn default() -> Self {
        Self { ptr: MarkedPtr::null(), _marker: Default::default() }
    }
}