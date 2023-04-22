use atomic::Ordering;

use crate::{internal::MarkedCntObjPtr, AcquireRetire, AcquiredPtr, CountedObject};

use super::hp_impl::{defer, HazardPointer};

pub struct GuardHP {
    is_protecting: bool,
}

impl GuardHP {
    pub fn protected() -> Self {
        Self {
            is_protecting: true,
        }
    }

    pub fn unprotected() -> Self {
        Self {
            is_protecting: false,
        }
    }
}

pub struct AcquiredPtrHP<T> {
    hazptr: Option<HazardPointer<'static>>,
    ptr: MarkedCntObjPtr<T>,
}

impl<T> AcquiredPtr<T> for AcquiredPtrHP<T> {
    unsafe fn deref_counted_ptr(&self) -> &MarkedCntObjPtr<T> {
        &self.ptr
    }

    unsafe fn deref_counted_ptr_mut(&mut self) -> &mut MarkedCntObjPtr<T> {
        &mut self.ptr
    }

    fn as_counted_ptr(&self) -> MarkedCntObjPtr<T> {
        self.ptr
    }

    fn null() -> Self {
        Self {
            hazptr: None,
            ptr: MarkedCntObjPtr::null(),
        }
    }

    fn is_null(&self) -> bool {
        self.ptr.is_null()
    }

    fn is_protected(&self) -> bool {
        self.hazptr.is_some() && !self.ptr.is_null()
    }

    fn clear_protection(&mut self) {
        if let Some(hazptr) = self.hazptr.as_mut() {
            hazptr.reset_protection();
        }
    }

    fn swap(p1: &mut Self, p2: &mut Self) {
        core::mem::swap(p1, p2);
    }

    fn eq(&self, other: &Self) -> bool {
        self.ptr.eq(&other.ptr)
    }
}

impl AcquireRetire for GuardHP {
    type AcquiredPtr<T> = AcquiredPtrHP<T>;

    fn handle() -> Self {
        Self::protected()
    }

    unsafe fn unprotected() -> Self {
        Self::unprotected()
    }

    fn create_object<T>(&self, obj: T) -> *mut crate::CountedObject<T> {
        let obj = CountedObject::new(obj);
        Box::into_raw(Box::new(obj))
    }

    fn acquire<T>(&self, link: &atomic::Atomic<MarkedCntObjPtr<T>>) -> Self::AcquiredPtr<T> {
        let mut ptr = link.load(Ordering::Relaxed);
        let mut hazptr = HazardPointer::default();

        loop {
            hazptr.protect_raw(ptr.unmarked());
            membarrier::light();
            let new_ptr = link.load(Ordering::Acquire);
            if ptr == new_ptr {
                break;
            }
            ptr = new_ptr;
        }
        AcquiredPtrHP {
            hazptr: Some(hazptr),
            ptr,
        }
    }

    fn reserve<T>(&self, ptr: *mut crate::CountedObject<T>) -> Self::AcquiredPtr<T> {
        let ptr = MarkedCntObjPtr::new(ptr);
        let mut hazptr = HazardPointer::default();
        hazptr.protect_raw(ptr.unmarked());
        membarrier::light();
        AcquiredPtrHP {
            hazptr: Some(hazptr),
            ptr,
        }
    }

    fn reserve_nothing<T>(&self) -> Self::AcquiredPtr<T> {
        AcquiredPtrHP::null()
    }

    fn protect_snapshot<T>(
        &self,
        link: &atomic::Atomic<MarkedCntObjPtr<T>>,
    ) -> Self::AcquiredPtr<T> {
        let mut ptr = link.load(Ordering::Relaxed);
        let mut hazptr = HazardPointer::default();

        loop {
            hazptr.protect_raw(ptr.unmarked());
            membarrier::light();
            let new_ptr = link.load(Ordering::Acquire);
            if ptr == new_ptr {
                break;
            }
            ptr = new_ptr;
        }
        AcquiredPtrHP {
            hazptr: Some(hazptr),
            ptr,
        }
    }

    fn reserve_snapshot<T>(&self, ptr: MarkedCntObjPtr<T>) -> Self::AcquiredPtr<T> {
        let mut hazptr = HazardPointer::default();
        hazptr.protect_raw(ptr.unmarked());
        membarrier::light();
        AcquiredPtrHP {
            hazptr: Some(hazptr),
            ptr,
        }
    }

    fn release(&self) {}

    unsafe fn delete_object<T>(&self, ptr: *mut crate::CountedObject<T>) {
        drop(Box::from_raw(ptr));
    }

    unsafe fn retire<T>(&self, ptr: *mut crate::CountedObject<T>, ret_type: crate::RetireType) {
        if self.is_protecting {
            let marked = MarkedCntObjPtr::new(ptr);
            defer(marked.unmarked(), move || {
                let inner_guard = Self::handle();
                inner_guard.eject(ptr, ret_type);
            });
        } else {
            self.eject(ptr, ret_type);
        }
    }
}
