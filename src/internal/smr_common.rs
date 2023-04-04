use crate::internal::utils::CountedObject;
use crate::internal::utils::EjectAction;

pub enum RetireType {
    DecrementStrongCount,
    DecrementWeakCount,
    Dispose,
}

/// A SMR-specific memory managing trait
pub trait AcquireRetire<T> {
    fn handle() -> Self;
    unsafe fn delete_object(&self, ptr: *mut CountedObject<T>);
    unsafe fn retire(&self, ptr: *mut CountedObject<T>, ret_type: RetireType);
}

pub(crate) unsafe fn dispose<T, S>(ptr: *mut CountedObject<T>, guard: &S)
where
    S: AcquireRetire<T>,
{
    assert!((*ptr).use_count() == 0);
    (*ptr).dispose();
    if (*ptr).release_weak_refs(1) {
        destroy(ptr, guard);
    }
}

pub(crate) unsafe fn destroy<T, S>(ptr: *mut CountedObject<T>, guard: &S)
where
    S: AcquireRetire<T>,
{
    assert!((*ptr).use_count() == 0);
    guard.delete_object(ptr);
}

pub(crate) unsafe fn retire<T, S>(ptr: *mut CountedObject<T>, ret_type: RetireType, guard: &S)
where
    S: AcquireRetire<T>,
{
    guard.retire(ptr, ret_type);
}

/// Perform an eject action. This can correspond to any action that
/// should be delayed until the ptr is no longer protected
pub(crate) unsafe fn eject<T, S>(ptr: *mut CountedObject<T>, ret_type: RetireType, guard: &S)
where
    S: AcquireRetire<T>,
{
    assert!(!ptr.is_null());

    match ret_type {
        RetireType::DecrementStrongCount => decrement_ref_cnt(ptr, guard),
        RetireType::DecrementWeakCount => decrement_weak_cnt(ptr, guard),
        RetireType::Dispose => dispose(ptr, guard),
    }
}

pub(crate) unsafe fn increment_ref_cnt<T, S>(ptr: *mut CountedObject<T>, guard: &S) -> bool
where
    S: AcquireRetire<T>,
{
    assert!(!ptr.is_null());
    (*ptr).add_refs(1)
}

pub(crate) unsafe fn increment_weak_cnt<T, S>(ptr: *mut CountedObject<T>, guard: &S) -> bool
where
    S: AcquireRetire<T>,
{
    assert!(!ptr.is_null());
    (*ptr).add_weak_refs(1)
}

pub(crate) unsafe fn decrement_ref_cnt<T, S>(ptr: *mut CountedObject<T>, guard: &S)
where
    S: AcquireRetire<T>,
{
    assert!(!ptr.is_null());
    assert!((*ptr).use_count() >= 1);
    let result = (*ptr).release_refs(1);

    match result {
        EjectAction::Nothing => {}
        EjectAction::Delay => retire(ptr, RetireType::Dispose, guard),
        EjectAction::Destroy => destroy(ptr, guard),
    }
}

pub(crate) unsafe fn decrement_weak_cnt<T, S>(ptr: *mut CountedObject<T>, guard: &S)
where
    S: AcquireRetire<T>,
{
    assert!(!ptr.is_null());
    assert!((*ptr).weak_count() >= 1);
    if (*ptr).release_weak_refs(1) {
        destroy(ptr, guard);
    }
}

pub(crate) unsafe fn delayed_decrement_ref_cnt<T, S>(ptr: *mut CountedObject<T>, guard: &S)
where
    S: AcquireRetire<T>,
{
    assert!((*ptr).use_count() >= 1);
    retire(ptr, RetireType::DecrementStrongCount, guard);
}

pub(crate) unsafe fn delayed_decrement_weak_cnt<T, S>(ptr: *mut CountedObject<T>, guard: &S)
where
    S: AcquireRetire<T>,
{
    assert!((*ptr).weak_count() >= 1);
    retire(ptr, RetireType::DecrementWeakCount, guard);
}
