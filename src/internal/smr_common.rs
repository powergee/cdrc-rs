use atomic::Atomic;

use crate::internal::utils::CountedObject;
use crate::internal::utils::EjectAction;
use crate::internal::utils::MarkedCntObjPtr;

pub enum RetireType {
    DecrementStrongCount,
    DecrementWeakCount,
    Dispose,
}

/// A SMR-specific acquired pointer trait
///
/// In most cases such as EBR, IBR and Hyaline,
/// AcquiredPtr is equivalent to a simple marked pointer
/// pointing a CountedObject<T>.
///
/// However, for some pointer-based SMR, `AcquiredPtr` should
/// contain other information like an index of a hazard slot.
/// For this reason, a type for acquired pointer must be
/// SMR-dependent, and every SMR must provide some
/// reasonable interfaces to access and manage this pointer.
pub trait AcquiredPtr<T> {
    /// Dereference to a immutable `MarkedCntObjPtr`.
    unsafe fn deref_counted_ptr(&self) -> &MarkedCntObjPtr<T>;
    /// Dereference to a mutable `MarkedCntObjPtr`.
    unsafe fn deref_counted_ptr_mut(&mut self) -> &mut MarkedCntObjPtr<T>;
    fn as_counted_ptr(&self) -> MarkedCntObjPtr<T>;
    fn is_null(&self) -> bool;
    fn is_protected(&self) -> bool;
    fn clear_protection(&mut self);
    fn swap(p1: &mut Self, p2: &mut Self);
    fn eq(&self, other: &Self) -> bool;
}

/// A SMR-specific memory managing trait
pub trait AcquireRetire {
    /// A SMR-specific acquired pointer trait
    ///
    /// For more information, read a comment on `AcquiredPtr<T>`.
    type AcquiredPtr<T>: AcquiredPtr<T>;

    /* SMR-specific protecting & releasing */

    fn handle() -> Self;
    fn create_object<T>(&self, obj: T) -> *mut CountedObject<T>;
    fn acquire<T>(&self, link: &Atomic<MarkedCntObjPtr<T>>) -> Self::AcquiredPtr<T>;
    /// Like `acquire`, but assuming that the caller already has a
    /// copy of the handle and knows that it is protected
    fn reserve<T>(&self, ptr: *mut CountedObject<T>) -> Self::AcquiredPtr<T>;
    /// Dummy function for when we need to conditionally reserve
    /// something, but might need to reserve nothing
    fn reserve_nothing<T>(&self) -> Self::AcquiredPtr<T>;
    fn protect_snapshot<T>(&self, link: &Atomic<MarkedCntObjPtr<T>>) -> Self::AcquiredPtr<T>;
    /// Like `protect_snapshot`, but assuming that the caller already has an
    /// another snapshot containing the pointer.
    fn reserve_snapshot<T>(&self, ptr: MarkedCntObjPtr<T>) -> Self::AcquiredPtr<T>;
    fn release(&self);
    unsafe fn delete_object<T>(&self, ptr: *mut CountedObject<T>);
    unsafe fn retire<T>(&self, ptr: *mut CountedObject<T>, ret_type: RetireType);

    /* Public interfaces for CDRC operations */

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
            EjectAction::Destroy => self.destroy(ptr),
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

    /* Interfaces to access & manage the acquired pointer */
}
