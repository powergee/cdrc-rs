use crate::internal::{AcquireRetire, AcquiredPtr, CountedObjPtr, CountedObject};

pub struct SnapshotPtr<T, Guard>
where
    Guard: AcquireRetire<T>,
{
    acquired: Guard::AcquiredPtr,
}

impl<T, Guard> SnapshotPtr<T, Guard>
where
    Guard: AcquireRetire<T>,
{
    pub fn new(acquired: Guard::AcquiredPtr) -> Self {
        Self { acquired }
    }

    pub(crate) unsafe fn deref_counted(&self, _: &Guard) -> &CountedObject<T> {
        self.acquired.deref_counted()
    }

    pub(crate) unsafe fn deref_counted_mut(&self, _: &Guard) -> &mut CountedObject<T> {
        self.acquired.deref_counted_mut()
    }

    pub unsafe fn deref_data(&self, guard: &Guard) -> &T {
        self.deref_counted(guard).data()
    }

    pub unsafe fn deref_data_mut(&self, guard: &Guard) -> &mut T {
        self.deref_counted_mut(guard).data_mut()
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

    pub fn as_counted_ptr(&self) -> CountedObjPtr<T> {
        self.acquired.as_counted_ptr()
    }

    pub fn is_protected(&self) -> bool {
        self.acquired.is_protected()
    }
}

impl<T, Guard> Drop for SnapshotPtr<T, Guard>
where
    Guard: AcquireRetire<T>,
{
    fn drop(&mut self) {
        if !self.is_null() && !self.acquired.is_protected() {
            self.clear(&Guard::handle())
        }
    }
}

impl<T, Guard> PartialEq for SnapshotPtr<T, Guard>
where
    Guard: AcquireRetire<T>,
{
    fn eq(&self, other: &Self) -> bool {
        self.acquired.eq(&other.acquired)
    }
}
