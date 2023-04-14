use std::mem;

use atomic::Ordering;

use crate::internal::smr_common::Handle;
use crate::internal::utils::CountedObject;
use crate::internal::{AcquireRetire, AcquiredPtr, MarkedCntObjPtr, RetireType};
use crate::EjectAction;

pub struct HandleEBR {}

impl Handle for HandleEBR {
    type Guard = GuardEBR;

    unsafe fn set_max_threads(_: usize) {}

    unsafe fn reset_registrations() {}

    fn register() -> Self {
        Self {}
    }

    fn pin(&self) -> Self::Guard {
        GuardEBR {
            guard: Some(crossbeam_epoch::pin()),
        }
    }
}

pub struct GuardEBR {
    guard: Option<crossbeam_epoch::Guard>,
}

impl GuardEBR {
    unsafe fn retire<T>(&self, ptr: *mut CountedObject<T>, ret_type: RetireType) {
        if let Some(guard) = &self.guard {
            guard.defer_unchecked(move || {
                let inner_guard = Self {
                    guard: Some(crossbeam_epoch::pin()),
                };
                inner_guard.eject(ptr, ret_type);
            });
        } else {
            self.eject(ptr, ret_type)
        }
    }

    unsafe fn dispose<T>(&self, ptr: *mut CountedObject<T>) {
        assert!((*ptr).use_count() == 0);
        (*ptr).dispose();
        if (*ptr).release_weak_refs(1) {
            self.destroy(ptr);
        }
    }

    unsafe fn destroy<T>(&self, ptr: *mut CountedObject<T>) {
        assert!((*ptr).use_count() == 0);
        self.delete_object(ptr);
    }

    /// Perform an eject action. This can correspond to any action that
    /// should be delayed until the ptr is no longer protected
    unsafe fn eject<T>(&self, ptr: *mut CountedObject<T>, ret_type: RetireType) {
        assert!(!ptr.is_null());

        match ret_type {
            RetireType::DecrementStrongCount => self.decrement_ref_cnt(ptr),
            RetireType::DecrementWeakCount => self.decrement_weak_cnt(ptr),
            RetireType::Dispose => self.dispose(ptr),
        }
    }

    unsafe fn delete_object<T>(&self, ptr: *mut CountedObject<T>) {
        drop(Box::from_raw(ptr));
    }
}

impl AcquireRetire for GuardEBR {
    type AcquiredPtr<T> = AcquiredPtrEBR<T>;

    unsafe fn unprotected<'g>() -> &'g Self {
        struct GuardWrapper(GuardEBR);
        unsafe impl Sync for GuardWrapper {}
        static UNPROTECTED: GuardWrapper = GuardWrapper(GuardEBR { guard: None });
        &UNPROTECTED.0
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
        self.reserve_snapshot(link.load(Ordering::Acquire))
    }

    fn reserve_snapshot<T>(&self, ptr: MarkedCntObjPtr<T>) -> Self::AcquiredPtr<T> {
        if !ptr.is_null() && unsafe { ptr.deref() }.use_count() == 0 {
            AcquiredPtrEBR(MarkedCntObjPtr::null())
        } else {
            AcquiredPtrEBR(ptr)
        }
    }

    fn release(&self) {
        // For EBR, there's no action which is equivalent to releasing.
    }

    unsafe fn increment_ref_cnt<T>(&self, ptr: *mut CountedObject<T>) -> bool {
        assert!(!ptr.is_null());
        (*ptr).add_refs(1)
    }

    unsafe fn increment_weak_cnt<T>(&self, ptr: *mut CountedObject<T>) -> bool {
        assert!(!ptr.is_null());
        (*ptr).add_weak_refs(1)
    }

    unsafe fn decrement_ref_cnt<T>(&self, ptr: *mut CountedObject<T>) {
        assert!(!ptr.is_null());
        assert!((*ptr).use_count() >= 1);
        let result = (*ptr).release_refs(1);

        match result {
            EjectAction::Nothing => {}
            EjectAction::Delay => self.retire(ptr, RetireType::Dispose),
            EjectAction::Destroy => {
                (*ptr).dispose();
                self.destroy(ptr);
            }
        }
    }

    unsafe fn decrement_weak_cnt<T>(&self, ptr: *mut CountedObject<T>) {
        assert!(!ptr.is_null());
        assert!((*ptr).weak_count() >= 1);
        if (*ptr).release_weak_refs(1) {
            self.destroy(ptr);
        }
    }

    unsafe fn delayed_decrement_ref_cnt<T>(&self, ptr: *mut CountedObject<T>) {
        assert!((*ptr).use_count() >= 1);
        self.retire(ptr, RetireType::DecrementStrongCount);
    }

    unsafe fn delayed_decrement_weak_cnt<T>(&self, ptr: *mut CountedObject<T>) {
        assert!((*ptr).weak_count() >= 1);
        self.retire(ptr, RetireType::DecrementWeakCount);
    }

    fn create_object<T>(&self, obj: T) -> *mut CountedObject<T> {
        let obj = CountedObject::new(obj);
        Box::into_raw(Box::new(obj))
    }
}

/// A marked pointer which is pointing a `CountedObjPtr<T>`.
///
/// We may want to use `crossbeam_ebr::Shared` as a `AcquiredPtr`,
/// but trait interfaces can be complicated because `crossbeam_ebr::Shared`
/// requires to specify a lifetime specifier.
pub struct AcquiredPtrEBR<T>(MarkedCntObjPtr<T>);

impl<T> AcquiredPtr<T> for AcquiredPtrEBR<T> {
    unsafe fn deref_counted_ptr(&self) -> &MarkedCntObjPtr<T> {
        &self.0
    }

    unsafe fn deref_counted_ptr_mut(&mut self) -> &mut MarkedCntObjPtr<T> {
        &mut self.0
    }

    fn as_counted_ptr(&self) -> MarkedCntObjPtr<T> {
        self.0
    }

    fn null() -> Self {
        Self(MarkedCntObjPtr::null())
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
