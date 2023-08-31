use std::mem;

use atomic::Ordering;

use crate::internal::utils::Counted;
use crate::internal::{Acquired, Guard, RetireType, TaggedCnt};

/// A tagged pointer which is pointing a `CountedObjPtr<T>`.
///
/// We may want to use `crossbeam_ebr::Shared` as a `Acquired`,
/// but trait interfaces can be complicated because `crossbeam_ebr::Shared`
/// requires to specify a lifetime specifier.
pub struct AcquiredEBR<T>(TaggedCnt<T>);

impl<T> Acquired<T> for AcquiredEBR<T> {
    #[inline(always)]
    fn ptr(&self) -> &TaggedCnt<T> {
        &self.0
    }

    #[inline(always)]
    fn ptr_mut(&mut self) -> &mut TaggedCnt<T> {
        &mut self.0
    }

    #[inline(always)]
    fn as_ptr(&self) -> TaggedCnt<T> {
        self.0
    }

    #[inline(always)]
    fn null() -> Self {
        Self(TaggedCnt::null())
    }

    #[inline(always)]
    fn is_null(&self) -> bool {
        self.0.is_null()
    }

    #[inline(always)]
    fn swap(p1: &mut Self, p2: &mut Self) {
        mem::swap(p1, p2);
    }

    #[inline(always)]
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }

    fn clear_protection(&mut self) {
        // No operation for EBR.
    }
}

pub struct GuardEBR {
    guard: Option<crossbeam_epoch::Guard>,
}

impl From<crossbeam_epoch::Guard> for GuardEBR {
    #[inline(always)]
    fn from(guard: crossbeam_epoch::Guard) -> Self {
        Self { guard: Some(guard) }
    }
}

impl Guard for GuardEBR {
    type Acquired<T> = AcquiredEBR<T>;

    #[inline(always)]
    fn new() -> Self {
        Self::from(crossbeam_epoch::pin())
    }

    #[inline(always)]
    fn create_object<T>(&self, obj: T) -> *mut Counted<T> {
        let obj = Counted::new(obj);
        Box::into_raw(Box::new(obj))
    }

    #[inline(always)]
    fn acquire<T>(&self, link: &atomic::Atomic<TaggedCnt<T>>) -> Self::Acquired<T> {
        AcquiredEBR(link.load(Ordering::Acquire))
    }

    #[inline(always)]
    fn protect_snapshot<T>(&self, link: &atomic::Atomic<TaggedCnt<T>>) -> Self::Acquired<T> {
        let ptr = link.load(Ordering::Acquire);
        if !ptr.is_null() && unsafe { ptr.deref() }.ref_count() == 0 {
            AcquiredEBR(TaggedCnt::null())
        } else {
            AcquiredEBR(ptr)
        }
    }

    #[inline(always)]
    unsafe fn delete_object<T>(&self, ptr: *mut Counted<T>) {
        drop(Box::from_raw(ptr));
    }

    #[inline(always)]
    unsafe fn retire<T>(&self, ptr: *mut Counted<T>, ret_type: RetireType) {
        debug_assert!(!ptr.is_null());
        let cnt = &mut *ptr;
        if let Some(guard) = &self.guard {
            guard.defer_unchecked(move || {
                let inner_guard = Self::without_epoch();
                inner_guard.eject(cnt, ret_type);
            });
        } else {
            self.eject(cnt, ret_type);
        }
    }

    unsafe fn without_epoch() -> Self {
        Self { guard: None }
    }

    fn clear(&mut self) {
        if let Some(guard) = &mut self.guard {
            guard.repin();
        }
    }
}
