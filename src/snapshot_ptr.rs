use crate::internal::{AcquireRetire, AcquiredPtr, MarkedCntObjPtr};

pub struct SnapshotPtr<'g, T: 'g, Guard: 'g>
where
    Guard: AcquireRetire,
{
    // Guard::AcquiredPtr is usually a wrapper struct
    // containing MarkedCntObjPtr.
    acquired: Guard::AcquiredPtr<T>,
    guard: &'g Guard,
}

impl<'g, T, Guard> SnapshotPtr<'g, T, Guard>
where
    Guard: AcquireRetire,
{
    pub fn new(acquired: Guard::AcquiredPtr<T>, guard: &'g Guard) -> Self {
        Self { acquired, guard }
    }
    
    pub fn null(guard: &'g Guard) -> Self {
        Self {
            acquired: <Guard as AcquireRetire>::AcquiredPtr::null(),
            guard
        }
    }

    pub fn clone(&self, guard: &'g Guard) -> Self {
        Self::new(guard.reserve_snapshot(self.as_counted_ptr()), guard)
    }

    /// # Safety
    /// TODO
    pub unsafe fn deref(&self) -> &'g T {
        self.acquired.deref_counted_ptr().deref().data()
    }

    /// # Safety
    /// TODO
    pub unsafe fn deref_mut(&mut self) -> &'g mut T {
        self.acquired.deref_counted_ptr_mut().deref_mut().data_mut()
    }

    pub unsafe fn as_ref(&self) -> Option<&'g T> {
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

    pub fn mark(&self) -> usize {
        self.as_counted_ptr().mark()
    }

    pub fn unmarked(self) -> Self {
        self.with_mark(0)
    }

    pub fn with_mark(mut self, mark: usize) -> Self {
        unsafe {
            self.acquired.deref_counted_ptr_mut().set_mark(mark);
        }
        self
    }
}

impl<'g, T, Guard> Drop for SnapshotPtr<'g, T, Guard>
where
    Guard: AcquireRetire,
{
    fn drop(&mut self) {
        if !self.is_null() && !self.acquired.is_protected() {
            self.clear(self.guard)
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
