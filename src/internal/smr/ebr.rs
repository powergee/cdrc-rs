use std::mem;

use atomic::Ordering;

use crate::internal::utils::CountedObject;
use crate::internal::{AcquireRetire, AcquiredPtr, MarkedCntObjPtr, RetireType};

pub struct GuardEBR {
    guard: Option<crossbeam_epoch::Guard>,
}

impl From<crossbeam_epoch::Guard> for GuardEBR {
    #[inline(always)]
    fn from(guard: crossbeam_epoch::Guard) -> Self {
        Self { guard: Some(guard) }
    }
}

/// A marked pointer which is pointing a `CountedObjPtr<T>`.
///
/// We may want to use `crossbeam_ebr::Shared` as a `AcquiredPtr`,
/// but trait interfaces can be complicated because `crossbeam_ebr::Shared`
/// requires to specify a lifetime specifier.
pub struct AcquiredPtrEBR<T>(MarkedCntObjPtr<T>);

impl<T> AcquiredPtr<T> for AcquiredPtrEBR<T> {
    #[inline(always)]
    unsafe fn deref_counted_ptr(&self) -> &MarkedCntObjPtr<T> {
        &self.0
    }

    #[inline(always)]
    unsafe fn deref_counted_ptr_mut(&mut self) -> &mut MarkedCntObjPtr<T> {
        &mut self.0
    }

    #[inline(always)]
    fn as_counted_ptr(&self) -> MarkedCntObjPtr<T> {
        self.0
    }

    #[inline(always)]
    fn null() -> Self {
        Self(MarkedCntObjPtr::null())
    }

    #[inline(always)]
    fn is_null(&self) -> bool {
        self.0.is_null()
    }

    #[inline(always)]
    fn is_protected(&self) -> bool {
        // We assume that a `Guard` is properly pinned.
        true
    }

    #[inline(always)]
    fn clear_protection(&mut self) {
        // For EBR, there's no action which unprotect a specific block.
    }

    #[inline(always)]
    fn swap(p1: &mut Self, p2: &mut Self) {
        mem::swap(p1, p2);
    }

    #[inline(always)]
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl AcquireRetire for GuardEBR {
    type AcquiredPtr<T> = AcquiredPtrEBR<T>;

    #[inline(always)]
    fn handle() -> Self {
        Self::from(crossbeam_epoch::pin())
    }

    #[inline(always)]
    fn create_object<T>(&self, obj: T) -> *mut CountedObject<T> {
        let obj = CountedObject::new(obj);
        Box::into_raw(Box::new(obj))
    }

    #[inline(always)]
    fn acquire<T>(&self, link: &atomic::Atomic<MarkedCntObjPtr<T>>) -> Self::AcquiredPtr<T> {
        AcquiredPtrEBR(link.load(Ordering::Acquire))
    }

    #[inline(always)]
    fn reserve<T>(&self, ptr: *mut CountedObject<T>) -> Self::AcquiredPtr<T> {
        AcquiredPtrEBR(MarkedCntObjPtr::new(ptr))
    }

    #[inline(always)]
    fn reserve_nothing<T>(&self) -> Self::AcquiredPtr<T> {
        AcquiredPtrEBR(MarkedCntObjPtr::null())
    }

    #[inline(always)]
    fn protect_snapshot<T>(
        &self,
        link: &atomic::Atomic<MarkedCntObjPtr<T>>,
    ) -> Self::AcquiredPtr<T> {
        self.reserve_snapshot(link.load(Ordering::Acquire))
    }

    #[inline(always)]
    fn reserve_snapshot<T>(&self, ptr: MarkedCntObjPtr<T>) -> Self::AcquiredPtr<T> {
        if !ptr.is_null() && unsafe { ptr.deref() }.use_count() == 0 {
            AcquiredPtrEBR(MarkedCntObjPtr::null())
        } else {
            AcquiredPtrEBR(ptr)
        }
    }

    #[inline(always)]
    fn release(&self) {
        // For EBR, there's no action which is equivalent to releasing.
    }

    #[inline(always)]
    unsafe fn delete_object<T>(&self, ptr: *mut CountedObject<T>) {
        drop(Box::from_raw(ptr));
    }

    #[inline(always)]
    unsafe fn retire<T>(&self, ptr: *mut CountedObject<T>, ret_type: RetireType) {
        if let Some(guard) = &self.guard {
            guard.defer_unchecked(move || {
                let inner_guard = Self::unprotected();
                inner_guard.eject(ptr, ret_type);
            });
        } else {
            self.eject(ptr, ret_type);
        }
    }

    unsafe fn unprotected() -> Self {
        Self { guard: None }
    }
}
