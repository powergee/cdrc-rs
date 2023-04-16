use crate::{
    internal::{AcquireRetire, Count, MarkedCntObjPtr, MarkedPtr},
    snapshot_ptr::SnapshotPtr,
    LocalPtr,
};

pub struct RcPtr<'g, T: 'g, Guard: 'g>
where
    Guard: AcquireRetire,
{
    ptr: MarkedCntObjPtr<T>,
    guard: &'g Guard,
}

impl<'g, T, Guard> RcPtr<'g, T, Guard>
where
    Guard: AcquireRetire,
{
    #[inline(always)]
    pub fn null(guard: &'g Guard) -> Self {
        Self::new_without_incr(MarkedCntObjPtr::null(), guard)
    }

    #[inline(always)]
    pub(crate) fn new_with_incr(ptr: MarkedCntObjPtr<T>, guard: &'g Guard) -> Self {
        let rc = Self { ptr, guard };
        if !ptr.is_null() {
            unsafe { guard.increment_ref_cnt(rc.ptr.unmarked()) };
        }
        rc
    }

    #[inline(always)]
    pub(crate) fn new_without_incr(ptr: MarkedCntObjPtr<T>, guard: &'g Guard) -> Self {
        Self { ptr, guard }
    }

    #[inline(always)]
    pub fn from_snapshot(ptr: &SnapshotPtr<'g, T, Guard>, guard: &'g Guard) -> Self {
        let rc = Self {
            ptr: ptr.as_counted_ptr(),
            guard,
        };
        if !ptr.is_null() {
            unsafe { guard.increment_ref_cnt(rc.ptr.unmarked()) };
        }
        rc
    }

    #[inline(always)]
    pub fn make_shared(obj: T, guard: &'g Guard) -> Self {
        let ptr = guard.create_object(obj);
        Self {
            ptr: MarkedCntObjPtr::new(ptr),
            guard,
        }
    }

    #[inline(always)]
    pub fn clone(&self, guard: &'g Guard) -> Self {
        let rc = Self {
            ptr: self.ptr,
            guard,
        };
        if !rc.ptr.is_null() {
            unsafe { guard.increment_ref_cnt(rc.ptr.unmarked()) };
        }
        rc
    }

    #[inline(always)]
    pub fn clear(&mut self, guard: &Guard) {
        if !self.ptr.is_null() {
            unsafe { guard.decrement_ref_cnt(self.ptr.unmarked()) };
        }
        self.ptr = MarkedPtr::null();
    }

    #[inline(always)]
    pub fn is_null(&self) -> bool {
        self.ptr.is_null()
    }

    #[inline(always)]
    pub unsafe fn as_ref(&self) -> Option<&'g T> {
        if self.is_null() {
            None
        } else {
            Some(unsafe { self.deref() })
        }
    }

    /// # Safety
    /// TODO
    #[inline(always)]
    pub unsafe fn deref(&self) -> &'g T {
        self.ptr.deref().data()
    }

    /// # Safety
    /// TODO
    #[inline(always)]
    pub unsafe fn deref_mut(&mut self) -> &'g mut T {
        self.ptr.deref_mut().data_mut()
    }

    #[inline(always)]
    pub fn use_count(&self) -> Count {
        unsafe { self.ptr.deref().use_count() }
    }

    #[inline(always)]
    pub fn weak_count(&self) -> Count {
        unsafe { self.ptr.deref().weak_count() }
    }

    #[inline(always)]
    pub fn release(mut self) -> MarkedCntObjPtr<T> {
        let res = self.ptr;
        self.ptr = MarkedCntObjPtr::null();
        res
    }

    #[inline(always)]
    pub fn as_counted_ptr(&self) -> MarkedCntObjPtr<T> {
        self.ptr
    }

    #[inline(always)]
    pub fn mark(&self) -> usize {
        self.ptr.mark()
    }

    #[inline(always)]
    pub fn unmarked(mut self) -> Self {
        self.ptr = MarkedCntObjPtr::new(self.ptr.unmarked());
        self
    }

    #[inline(always)]
    pub fn with_mark(mut self, mark: usize) -> Self {
        self.ptr.set_mark(mark);
        self
    }

    #[inline(always)]
    pub fn as_usize(&self) -> usize {
        self.ptr.as_usize()
    }

    #[inline(always)]
    pub fn is_protected(&self) -> bool {
        false
    }
}

impl<'g, T, Guard> Drop for RcPtr<'g, T, Guard>
where
    Guard: AcquireRetire,
{
    #[inline(always)]
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            self.clear(self.guard);
        }
    }
}

impl<'g, T, Guard> PartialEq for RcPtr<'g, T, Guard>
where
    Guard: AcquireRetire,
{
    #[inline(always)]
    fn eq(&self, other: &Self) -> bool {
        self.ptr == other.ptr
    }
}

impl<'g, T, Guard> LocalPtr<'g, T, Guard> for RcPtr<'g, T, Guard>
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
    fn as_rc(self) -> RcPtr<'g, T, Guard> {
        self
    }
}
