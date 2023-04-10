use crate::internal::{AcquireRetire, AcquiredPtr, MarkedCntObjPtr, CountedObject};

pub struct SnapshotPtr<T, Guard>
where
    Guard: AcquireRetire,
{
    // Guard::AcquiredPtr is usually a wrapper struct
    // containing MarkedCntObjPtr.
    acquired: Guard::AcquiredPtr<T>,
}

impl<T, Guard> SnapshotPtr<T, Guard>
where
    Guard: AcquireRetire,
{
    pub fn new(acquired: Guard::AcquiredPtr<T>) -> Self {
        Self { acquired }
    }

    pub(crate) unsafe fn deref_counted(&self, _: &Guard) -> &CountedObject<T> {
        self.acquired.deref_counted()
    }

    pub(crate) unsafe fn deref_counted_mut(&mut self, _: &Guard) -> &mut CountedObject<T> {
        self.acquired.deref_counted_mut()
    }

    /// # Safety
    /// TODO
    pub unsafe fn deref(&self, guard: &Guard) -> &T {
        self.deref_counted(guard).data()
    }

    /// # Safety
    /// TODO
    pub unsafe fn deref_mut(&mut self, guard: &Guard) -> &mut T {
        self.deref_counted_mut(guard).data_mut()
    }

    pub fn as_ref(&self, guard: &Guard) -> Option<&T> {
        if self.is_null() {
            None
        } else {
            Some(unsafe { self.deref(guard) })
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

impl<T, Guard> Drop for SnapshotPtr<T, Guard>
where
    Guard: AcquireRetire,
{
    fn drop(&mut self) {
        if !self.is_null() && !self.acquired.is_protected() {
            self.clear(&Guard::handle())
        }
    }
}

impl<T, Guard> PartialEq for SnapshotPtr<T, Guard>
where
    Guard: AcquireRetire,
{
    fn eq(&self, other: &Self) -> bool {
        self.acquired.eq(&other.acquired)
    }
}
