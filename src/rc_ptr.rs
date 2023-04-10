use std::marker::PhantomData;

use crate::{
    internal::{AcquireRetire, Count, MarkedCntObjPtr, MarkedPtr},
    snapshot_ptr::SnapshotPtr,
};

pub struct RcPtr<'g, T: 'g, Guard: 'g>
where
    Guard: AcquireRetire,
{
    ptr: MarkedCntObjPtr<T>,
    _marker: PhantomData<(&'g (), Guard)>,
}

impl<'g, T, Guard> RcPtr<'g, T, Guard>
where
    Guard: AcquireRetire,
{
    pub(crate) fn new_with_incr(ptr: MarkedCntObjPtr<T>, guard: &'g Guard) -> Self {
        let rc = Self {
            ptr,
            _marker: PhantomData,
        };
        if !ptr.is_null() {
            unsafe { guard.increment_ref_cnt(rc.ptr.unmarked()) };
        }
        rc
    }

    pub(crate) fn new_without_incr(ptr: MarkedCntObjPtr<T>) -> Self {
        Self {
            ptr,
            _marker: PhantomData,
        }
    }

    pub fn from_snapshot(ptr: &SnapshotPtr<'g, T, Guard>, guard: &'g Guard) -> Self {
        let rc = Self {
            ptr: ptr.as_counted_ptr(),
            _marker: PhantomData,
        };
        if !ptr.is_null() {
            unsafe { guard.increment_ref_cnt(rc.ptr.unmarked()) };
        }
        rc
    }

    pub fn make_shared(obj: T, guard: &'g Guard) -> Self {
        let ptr = guard.create_object(obj);
        Self {
            ptr: MarkedCntObjPtr::new(ptr),
            _marker: PhantomData,
        }
    }

    pub fn clone(&self, guard: &'g Guard) -> Self {
        let rc = Self {
            ptr: self.ptr,
            _marker: PhantomData,
        };
        if !rc.ptr.is_null() {
            unsafe { guard.increment_ref_cnt(rc.ptr.unmarked()) };
        }
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

    pub unsafe fn as_ref(&self) -> Option<&'g T> {
        if self.is_null() {
            None
        } else {
            Some(unsafe { self.deref() })
        }
    }

    /// # Safety
    /// TODO
    pub unsafe fn deref(&self) -> &'g T {
        self.ptr.deref().data()
    }

    /// # Safety
    /// TODO
    pub unsafe fn deref_mut(&mut self) -> &'g mut T {
        self.ptr.deref_mut().data_mut()
    }

    pub fn use_count(&self) -> Count {
        unsafe { self.ptr.deref().use_count() }
    }

    pub fn weak_count(&self) -> Count {
        unsafe { self.ptr.deref().weak_count() }
    }

    pub fn release(mut self) -> MarkedCntObjPtr<T> {
        let res = self.ptr;
        self.ptr = MarkedCntObjPtr::null();
        res
    }

    pub(crate) fn as_counted_ptr(&self) -> MarkedCntObjPtr<T> {
        self.ptr
    }

    pub fn mark(&self) -> usize {
        self.ptr.mark()
    }

    pub fn unmarked(mut self) -> Self {
        self.ptr = MarkedCntObjPtr::new(self.ptr.unmarked());
        self
    }

    pub fn with_mark(mut self, mark: usize) -> Self {
        self.ptr.set_mark(mark);
        self
    }

    pub fn eq_without_tag(&self, rhs: &Self) -> bool {
        self.as_counted_ptr().unmarked() == rhs.as_counted_ptr().unmarked()
    }

    pub fn is_protected(&self) -> bool {
        false
    }
}

impl<'g, T, Guard> Drop for RcPtr<'g, T, Guard>
where
    Guard: AcquireRetire,
{
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            self.clear(&Guard::handle());
        }
    }
}

impl<'g, T, Guard> PartialEq for RcPtr<'g, T, Guard>
where
    Guard: AcquireRetire,
{
    fn eq(&self, other: &Self) -> bool {
        self.ptr == other.ptr
    }
}

impl<'g, T, Guard> Default for RcPtr<'g, T, Guard>
where
    Guard: AcquireRetire,
{
    fn default() -> Self {
        Self {
            ptr: MarkedPtr::null(),
            _marker: Default::default(),
        }
    }
}
