use crate::internal::{AcquireRetire, AcquiredPtr, CountedObject};

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
            unsafe { guard.decrement_ref_cnt(self.acquired.as_unmarked()) }
        }
        self.acquired.clear_protection();
    }
}

impl<T, S> Drop for SnapshotPtr<T, S>
where
    S: AcquireRetire<T>,
{
    fn drop(&mut self) {
        self.clear(&S::handle())
    }
}

impl<T, S> PartialEq for SnapshotPtr<T, S>
where
    S: AcquireRetire<T>,
{
    fn eq(&self, other: &Self) -> bool {
        self.acquired.eq(&other.acquired)
    }
}
