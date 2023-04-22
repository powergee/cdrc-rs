use atomic::Ordering;

use crate::{internal::MarkedCntObjPtr, AcquireRetire, AcquiredPtr, CountedObject};

use super::hp_impl::{defer, HazardPointer};

pub struct GuardHP {
    is_protecting: bool,
}

impl GuardHP {
    #[inline(always)]
    pub fn protected() -> Self {
        Self {
            is_protecting: true,
        }
    }

    #[inline(always)]
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
    #[inline(always)]
    unsafe fn deref_counted_ptr(&self) -> &MarkedCntObjPtr<T> {
        &self.ptr
    }

    #[inline(always)]
    unsafe fn deref_counted_ptr_mut(&mut self) -> &mut MarkedCntObjPtr<T> {
        &mut self.ptr
    }

    #[inline(always)]
    fn as_counted_ptr(&self) -> MarkedCntObjPtr<T> {
        self.ptr
    }

    #[inline(always)]
    fn null() -> Self {
        Self {
            hazptr: None,
            ptr: MarkedCntObjPtr::null(),
        }
    }

    #[inline(always)]
    fn is_null(&self) -> bool {
        self.ptr.is_null()
    }

    #[inline(always)]
    fn is_protected(&self) -> bool {
        self.hazptr.is_some() && !self.ptr.is_null()
    }

    #[inline(always)]
    fn clear_protection(&mut self) {
        if let Some(hazptr) = self.hazptr.as_mut() {
            hazptr.reset_protection();
        }
    }

    #[inline(always)]
    fn swap(p1: &mut Self, p2: &mut Self) {
        core::mem::swap(p1, p2);
    }

    #[inline(always)]
    fn eq(&self, other: &Self) -> bool {
        self.ptr.eq(&other.ptr)
    }
}

impl AcquireRetire for GuardHP {
    type AcquiredPtr<T> = AcquiredPtrHP<T>;

    #[inline(always)]
    fn handle() -> Self {
        Self::protected()
    }

    #[inline(always)]
    unsafe fn unprotected() -> Self {
        Self::unprotected()
    }

    #[inline(always)]
    fn create_object<T>(&self, obj: T) -> *mut crate::CountedObject<T> {
        let obj = CountedObject::new(obj);
        Box::into_raw(Box::new(obj))
    }

    #[inline(always)]
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

    #[inline(always)]
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

    #[inline(always)]
    fn reserve_nothing<T>(&self) -> Self::AcquiredPtr<T> {
        AcquiredPtrHP::null()
    }

    #[inline(always)]
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

    #[inline(always)]
    fn reserve_snapshot<T>(&self, ptr: MarkedCntObjPtr<T>) -> Self::AcquiredPtr<T> {
        let mut hazptr = HazardPointer::default();
        hazptr.protect_raw(ptr.unmarked());
        membarrier::light();
        AcquiredPtrHP {
            hazptr: Some(hazptr),
            ptr,
        }
    }

    #[inline(always)]
    fn release(&self) {}

    #[inline(always)]
    unsafe fn delete_object<T>(&self, ptr: *mut crate::CountedObject<T>) {
        drop(Box::from_raw(ptr));
    }

    #[inline(always)]
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
