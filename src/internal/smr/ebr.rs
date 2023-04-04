use std::marker::PhantomData;

use crossbeam_epoch::Guard;

use crate::internal::utils::CountedObject;
use crate::internal::{eject, AcquireRetire, RetireType};

pub struct GuardEBR<T> {
    guard: Guard,
    _marker: PhantomData<T>,
}

impl<T> From<Guard> for GuardEBR<T> {
    fn from(guard: Guard) -> Self {
        Self {
            guard,
            _marker: PhantomData,
        }
    }
}

impl<T> AcquireRetire<T> for GuardEBR<T> {
    fn handle() -> Self {
        Self::from(crossbeam_epoch::pin())
    }

    unsafe fn delete_object(&self, ptr: *mut CountedObject<T>) {
        drop(Box::from_raw(ptr));
    }

    unsafe fn retire(&self, ptr: *mut CountedObject<T>, ret_type: RetireType) {
        self.guard.defer_unchecked(move || {
            let inner_guard = Self::handle();
            eject(ptr, ret_type, &inner_guard);
        });
    }
}
