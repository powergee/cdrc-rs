use std::mem;

use atomic::Ordering;

use crate::internal::utils::CountedObject;
use crate::internal::{AcquireRetire, AcquiredPtr, MarkedCntObjPtr, RetireType};

pub struct GuardEBR {
    guard: crossbeam_epoch::Guard,
}

impl From<crossbeam_epoch::Guard> for GuardEBR {
    fn from(guard: crossbeam_epoch::Guard) -> Self {
        Self { guard }
    }
}

/// A marked pointer which is pointing a `CountedObjPtr<T>`.
///
/// We may want to use `crossbeam_ebr::Shared` as a `AcquiredPtr`,
/// but trait interfaces can be complicated because `crossbeam_ebr::Shared`
/// requires to specify a lifetime specifier.
pub struct AcquiredPtrEBR<T>(MarkedCntObjPtr<T>);

impl<T> AcquiredPtr<T> for AcquiredPtrEBR<T> {
    unsafe fn deref_counted<'g>(&self) -> &'g CountedObject<T> {
        self.0.deref()
    }

    unsafe fn deref_counted_mut<'g>(&mut self) -> &'g mut CountedObject<T> {
        self.0.deref_mut()
    }

    fn as_counted_ptr(&self) -> MarkedCntObjPtr<T> {
        self.0
    }

    fn is_null(&self) -> bool {
        self.0.is_null()
    }

    fn is_protected(&self) -> bool {
        // We assume that a `Guard` is properly pinned.
        true
    }

    fn clear_protection(&mut self) {
        // For EBR, there's no action which unprotect a specific block.
    }

    fn swap(p1: &mut Self, p2: &mut Self) {
        mem::swap(p1, p2);
    }

    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl AcquireRetire for GuardEBR {
    type AcquiredPtr<T> = AcquiredPtrEBR<T>;

    fn handle() -> Self {
        Self::from(crossbeam_epoch::pin())
    }

    fn create_object<T>(&self, obj: T) -> *mut CountedObject<T> {
        let obj = CountedObject::new(obj);
        Box::into_raw(Box::new(obj))
    }

    fn acquire<T>(&self, link: &atomic::Atomic<MarkedCntObjPtr<T>>) -> Self::AcquiredPtr<T> {
        AcquiredPtrEBR(link.load(Ordering::Acquire))
    }

    fn reserve<T>(&self, ptr: *mut CountedObject<T>) -> Self::AcquiredPtr<T> {
        AcquiredPtrEBR(MarkedCntObjPtr::new(ptr))
    }

    fn reserve_nothing<T>(&self) -> Self::AcquiredPtr<T> {
        AcquiredPtrEBR(MarkedCntObjPtr::null())
    }

    fn protect_snapshot<T>(
        &self,
        link: &atomic::Atomic<MarkedCntObjPtr<T>>,
    ) -> Self::AcquiredPtr<T> {
        let ptr = link.load(Ordering::Acquire);
        if !ptr.is_null() && unsafe { ptr.deref() }.use_count() == 0 {
            AcquiredPtrEBR(MarkedCntObjPtr::null())
        } else {
            AcquiredPtrEBR(ptr)
        }
    }

    fn release(&self) {
        // For EBR, there's no action which is equivalent to releasing.
    }

    unsafe fn delete_object<T>(&self, ptr: *mut CountedObject<T>) {
        drop(Box::from_raw(ptr));
    }

    unsafe fn retire<T>(&self, ptr: *mut CountedObject<T>, ret_type: RetireType) {
        self.guard.defer_unchecked(move || {
            let inner_guard = Self::handle();
            inner_guard.eject(ptr, ret_type);
        });
    }
}
