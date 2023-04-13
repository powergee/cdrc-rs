use atomic::Atomic;

use crate::internal::utils::CountedObject;
use crate::internal::utils::MarkedCntObjPtr;

#[derive(Clone, Copy)]
pub enum RetireType {
    DecrementStrongCount,
    DecrementWeakCount,
    Dispose,
}

/// A SMR-specific handle trait
///
/// `Handle` is responsible for configuring the SMR options,
/// registering each thread and creating `Guard` before entering
/// a critical section. While registering, a thread-local entity
/// must be properly initialized according to the SMR.
pub trait Handle {
    /// A SMR-specific critical section guard trait
    type Guard: AcquireRetire;

    /// Set the maximum allowed number of participants.
    /// This function must be called before actually
    /// using the garbage collector.
    unsafe fn set_max_threads(threads: usize);
    /// Unregister all participants and re-initialize
    /// all thread-local entities if necessary.
    unsafe fn reset_registrations();

    /// Register current thread as a participant.
    fn register() -> Self;
    /// Pin current thread, which involves begining its
    /// critical section.
    fn pin(&self) -> Self::Guard;
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

/// A SMR-specific critical section guard trait
pub trait AcquireRetire {
    /// A SMR-specific acquired pointer trait
    ///
    /// For more information, read a comment on `AcquiredPtr<T>`.
    type AcquiredPtr<T>: AcquiredPtr<T>;

    /* SMR-specific protecting & releasing */

    unsafe fn unprotected<'g>() -> &'g Self;
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

    unsafe fn increment_ref_cnt<T>(&self, ptr: *mut CountedObject<T>) -> bool;
    unsafe fn increment_weak_cnt<T>(&self, ptr: *mut CountedObject<T>) -> bool;
    unsafe fn decrement_ref_cnt<T>(&self, ptr: *mut CountedObject<T>);
    unsafe fn decrement_weak_cnt<T>(&self, ptr: *mut CountedObject<T>);
    unsafe fn delayed_decrement_ref_cnt<T>(&self, ptr: *mut CountedObject<T>);
    unsafe fn delayed_decrement_weak_cnt<T>(&self, ptr: *mut CountedObject<T>);
}
