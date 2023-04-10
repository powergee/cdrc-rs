use std::marker::PhantomData;

use crate::internal::{AcquireRetire, AcquiredPtr, CountedObject, MarkedCntObjPtr};

pub struct SnapshotPtr<'g, T: 'g, Guard: 'g>
where
    Guard: AcquireRetire,
{
    // Guard::AcquiredPtr is usually a wrapper struct
    // containing MarkedCntObjPtr.
    acquired: Guard::AcquiredPtr<T>,
    _marker: PhantomData<&'g ()>,
}

impl<'g, T, Guard> SnapshotPtr<'g, T, Guard>
where
    Guard: AcquireRetire,
{
    pub fn new(acquired: Guard::AcquiredPtr<T>) -> Self {
        Self {
            acquired,
            _marker: PhantomData
        }
    }

    pub(crate) unsafe fn deref_counted(&self) -> &'g CountedObject<T> {
        self.acquired.deref_counted()
    }

    pub(crate) unsafe fn deref_counted_mut(&mut self) -> &'g mut CountedObject<T> {
        self.acquired.deref_counted_mut()
    }

    /// # Safety
    /// TODO
    pub unsafe fn deref(&self) -> &'g T {
        self.deref_counted().data()
    }

    /// # Safety
    /// TODO
    pub unsafe fn deref_mut(&mut self) -> &'g mut T {
        self.deref_counted_mut().data_mut()
    }

    pub fn as_ref(&self) -> Option<&'g T> {
        if self.is_null() {
            None
        } else {
            Some(unsafe { self.deref() })
        }
    }

    pub fn is_null(&self) -> bool {
        self.acquired.is_null()
    }

    pub fn clear(&mut self, guard: &Guard) {
        if !self.is_null() && !self.acquired.is_protected() {
            unsafe { guard.decrement_ref_cnt(self.acquired.as_counted_ptr().unmarked()) }
        }
        self.acquired.clear_protection();
    }

    pub fn as_counted_ptr(&self) -> MarkedCntObjPtr<T> {
        self.acquired.as_counted_ptr()
    }

    pub fn is_protected(&self) -> bool {
        self.acquired.is_protected()
    }
}

impl<'g, T, Guard> Drop for SnapshotPtr<'g, T, Guard>
where
    Guard: AcquireRetire,
{
    fn drop(&mut self) {
        if !self.is_null() && !self.acquired.is_protected() {
            self.clear(&Guard::handle())
        }
    }
}

impl<'g, T, Guard> PartialEq for SnapshotPtr<'g, T, Guard>
where
    Guard: AcquireRetire,
{
    fn eq(&self, other: &Self) -> bool {
        self.acquired.eq(&other.acquired)
    }
}
