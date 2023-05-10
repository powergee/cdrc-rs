use crate::{
    internal::{AcquireRetire, AcquiredPtr, MarkedCntObjPtr},
    LocalPtr, RcPtr,
};

pub struct SnapshotPtr<'g, T: 'g, Guard: 'g>
where
    Guard: AcquireRetire,
{
    // Guard::AcquiredPtr is usually a wrapper struct
    // containing MarkedCntObjPtr.
    pub(crate) acquired: Guard::AcquiredPtr<T>,
    guard: &'g Guard,
}

impl<'g, T, Guard> SnapshotPtr<'g, T, Guard>
where
    Guard: AcquireRetire,
{
    #[inline(always)]
    pub fn new(acquired: Guard::AcquiredPtr<T>, guard: &'g Guard) -> Self {
        Self { acquired, guard }
    }

    #[inline(always)]
    pub fn null(guard: &'g Guard) -> Self {
        Self {
            acquired: <Guard as AcquireRetire>::AcquiredPtr::null(),
            guard,
        }
    }

    #[inline(always)]
    pub fn clone(&self, guard: &'g Guard) -> Self {
        Self::new(guard.reserve_snapshot(self.as_counted_ptr()), guard)
    }

    /// # Safety
    /// TODO
    #[inline(always)]
    pub unsafe fn deref(&self) -> &'g T {
        self.acquired.deref_counted_ptr().deref().data()
    }

    /// # Safety
    /// TODO
    #[inline(always)]
    pub unsafe fn deref_mut(&mut self) -> &'g mut T {
        self.acquired.deref_counted_ptr_mut().deref_mut().data_mut()
    }

    #[inline(always)]
    pub unsafe fn as_ref(&self) -> Option<&'g T> {
        if self.is_null() {
            None
        } else {
            Some(unsafe { self.deref() })
        }
    }

    #[inline(always)]
    pub fn is_null(&self) -> bool {
        self.acquired.is_null()
    }

    #[inline(always)]
    pub fn clear(&mut self, guard: &Guard) {
        if !self.is_null() && !self.acquired.is_protected() {
            unsafe { guard.decrement_ref_cnt(self.acquired.as_counted_ptr().unmarked()) }
        }
        self.acquired.clear_protection();
    }

    #[inline(always)]
    pub fn as_counted_ptr(&self) -> MarkedCntObjPtr<T> {
        self.acquired.as_counted_ptr()
    }

    #[inline(always)]
    pub fn is_protected(&self) -> bool {
        self.acquired.is_protected()
    }

    #[inline(always)]
    pub fn mark(&self) -> usize {
        self.as_counted_ptr().mark()
    }

    #[inline(always)]
    pub fn unmarked(self) -> Self {
        self.with_mark(0)
    }

    #[inline(always)]
    pub fn with_mark(mut self, mark: usize) -> Self {
        unsafe {
            self.acquired.deref_counted_ptr_mut().set_mark(mark);
        }
        self
    }

    #[inline(always)]
    pub fn as_usize(&self) -> usize {
        self.as_counted_ptr().as_usize()
    }
}

impl<'g, T, Guard> Drop for SnapshotPtr<'g, T, Guard>
where
    Guard: AcquireRetire,
{
    #[inline(always)]
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
    #[inline(always)]
    fn eq(&self, other: &Self) -> bool {
        self.acquired.eq(&other.acquired)
    }
}

impl<'g, T, Guard> LocalPtr<'g, T, Guard> for SnapshotPtr<'g, T, Guard>
where
    Guard: AcquireRetire,
{
    #[inline(always)]
    fn is_null(&self) -> bool {
        self.is_null()
    }

    #[inline(always)]
    unsafe fn as_ref(&self) -> Option<&'g T> {
        self.as_ref()
    }

    #[inline(always)]
    unsafe fn deref(&self) -> &'g T {
        self.deref()
    }

    #[inline(always)]
    unsafe fn deref_mut(&mut self) -> &'g mut T {
        self.deref_mut()
    }

    #[inline(always)]
    fn as_counted_ptr(&self) -> MarkedCntObjPtr<T> {
        self.as_counted_ptr()
    }

    #[inline(always)]
    fn is_protected(&self) -> bool {
        self.is_protected()
    }

    #[inline(always)]
    fn as_usize(&self) -> usize {
        self.as_usize()
    }

    #[inline(always)]
    fn mark(&self) -> usize {
        self.mark()
    }

    #[inline(always)]
    fn with_mark(self, mark: usize) -> Self {
        self.with_mark(mark)
    }

    #[inline(always)]
    fn unmarked(self) -> Self {
        self.unmarked()
    }

    #[inline(always)]
    fn clone(&self, guard: &'g Guard) -> Self {
        self.clone(guard)
    }

    #[inline(always)]
    fn as_rc(self) -> crate::RcPtr<'g, T, Guard> {
        RcPtr::from_snapshot(&self, self.guard)
    }
}
