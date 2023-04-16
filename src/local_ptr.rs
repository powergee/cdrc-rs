use crate::{internal::MarkedCntObjPtr, AcquireRetire, RcPtr};

/// Common interfaces which is compatible with both
/// `RcPtr` and `SnapshotPtr`
pub trait LocalPtr<'g, T, Guard>
where
    Guard: AcquireRetire,
{
    fn is_null(&self) -> bool;
    unsafe fn as_ref(&self) -> Option<&'g T>;
    unsafe fn deref(&self) -> &'g T;
    unsafe fn deref_mut(&mut self) -> &'g mut T;
    fn as_counted_ptr(&self) -> MarkedCntObjPtr<T>;
    fn is_protected(&self) -> bool;
    fn as_usize(&self) -> usize;
    fn mark(&self) -> usize;
    fn with_mark(self, mark: usize) -> Self;
    fn unmarked(self) -> Self;
    fn clone(&self, guard: &'g Guard) -> Self;
    fn as_rc(self) -> RcPtr<'g, T, Guard>;
}
