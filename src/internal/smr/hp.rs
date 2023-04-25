use atomic::Ordering;

use crate::{internal::MarkedCntObjPtr, AcquireRetire, AcquiredPtr, CountedObject};

use super::hp_impl::{defer, HazardPointer};

pub struct GuardHP {}

pub enum AcquiredPtrHP<T> {
    ProtectedIndependently {
        hazptr: HazardPointer<'static>,
        ptr: MarkedCntObjPtr<T>,
    },
    Unprotected {
        ptr: MarkedCntObjPtr<T>,
    },
}

impl<T> AcquiredPtr<T> for AcquiredPtrHP<T> {
    #[inline(always)]
    unsafe fn deref_counted_ptr(&self) -> &MarkedCntObjPtr<T> {
        match self {
            AcquiredPtrHP::ProtectedIndependently { ptr, .. }
            | AcquiredPtrHP::Unprotected { ptr } => ptr,
        }
    }

    #[inline(always)]
    unsafe fn deref_counted_ptr_mut(&mut self) -> &mut MarkedCntObjPtr<T> {
        match self {
            AcquiredPtrHP::ProtectedIndependently { ptr, .. }
            | AcquiredPtrHP::Unprotected { ptr } => ptr,
        }
    }

    #[inline(always)]
    fn as_counted_ptr(&self) -> MarkedCntObjPtr<T> {
        match self {
            AcquiredPtrHP::ProtectedIndependently { ptr, .. }
            | AcquiredPtrHP::Unprotected { ptr } => ptr.clone(),
        }
    }

    #[inline(always)]
    fn null() -> Self {
        AcquiredPtrHP::Unprotected {
            ptr: MarkedCntObjPtr::null(),
        }
    }

    #[inline(always)]
    fn is_null(&self) -> bool {
        self.as_counted_ptr().is_null()
    }

    #[inline(always)]
    fn is_protected(&self) -> bool {
        match self {
            AcquiredPtrHP::ProtectedIndependently { .. } => true,
            AcquiredPtrHP::Unprotected { .. } => false,
        }
    }

    #[inline(always)]
    fn swap(p1: &mut Self, p2: &mut Self) {
        core::mem::swap(p1, p2);
    }

    #[inline(always)]
    fn eq(&self, other: &Self) -> bool {
        self.as_counted_ptr().eq(&other.as_counted_ptr())
    }
}

impl AcquireRetire for GuardHP {
    type AcquiredPtr<T> = AcquiredPtrHP<T>;

    #[inline(always)]
    fn handle() -> Self {
        GuardHP {}
    }

    #[inline(always)]
    unsafe fn unprotected() -> Self {
        GuardHP {}
    }

    #[inline(always)]
    fn create_object<T>(&self, obj: T) -> *mut crate::CountedObject<T> {
        let obj = CountedObject::new(obj);
        Box::into_raw(Box::new(obj))
    }

    #[inline(always)]
    fn acquire<T>(&self, link: &atomic::Atomic<MarkedCntObjPtr<T>>) -> Self::AcquiredPtr<T> {
        let hazptr = HazardPointer::default();
        let mut ptr = link.load(Ordering::Relaxed);
        loop {
            hazptr.protect_raw(ptr.unmarked());
            membarrier::light();
            let new_ptr = link.load(Ordering::Acquire);
            if ptr == new_ptr {
                break;
            }
            ptr = new_ptr;
        }
        AcquiredPtrHP::ProtectedIndependently { hazptr, ptr }
    }

    #[inline(always)]
    fn reserve<T>(&self, ptr: *mut crate::CountedObject<T>) -> Self::AcquiredPtr<T> {
        let hazptr = HazardPointer::default();
        let ptr = MarkedCntObjPtr::new(ptr);
        hazptr.protect_raw(ptr.unmarked());
        membarrier::light();
        AcquiredPtrHP::ProtectedIndependently { hazptr, ptr }
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
        let hazptr = HazardPointer::default();
        let mut ptr = link.load(Ordering::Relaxed);
        loop {
            hazptr.protect_raw(ptr.unmarked());
            membarrier::light();
            let new_ptr = link.load(Ordering::Acquire);
            if ptr == new_ptr {
                break;
            }
            ptr = new_ptr;
        }
        AcquiredPtrHP::ProtectedIndependently { hazptr, ptr }
    }

    #[inline(always)]
    fn reserve_snapshot<T>(&self, ptr: &Self::AcquiredPtr<T>) -> Self::AcquiredPtr<T> {
        match ptr {
            AcquiredPtrHP::ProtectedIndependently { hazptr, ptr } => {
                AcquiredPtrHP::ProtectedIndependently {
                    hazptr: hazptr.clone(),
                    ptr: ptr.clone(),
                }
            }
            AcquiredPtrHP::Unprotected { ptr } => AcquiredPtrHP::Unprotected { ptr: ptr.clone() },
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
        let marked = MarkedCntObjPtr::new(ptr);
        defer(marked.unmarked(), move || {
            let inner_guard = Self::unprotected();
            inner_guard.eject(ptr, ret_type);
        });
    }
}
