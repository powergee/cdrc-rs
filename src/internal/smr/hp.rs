use atomic::Ordering;

use crate::{internal::MarkedCntObjPtr, AcquireRetire, AcquiredPtr, CountedObject};

use super::hp_impl::{HazardPointer, Thread, DEFAULT_THREAD};

pub struct GuardHP {
    thread: *const Thread,
}

impl GuardHP {
    #[inline]
    fn thread(&self) -> &Thread {
        unsafe { &*self.thread }
    }

    #[inline]
    fn acquire_shield(&self) -> HazardPointer {
        HazardPointer::new(self.thread())
    }
}

pub enum AcquiredPtrHP<T> {
    ProtectedIndependently {
        hazptr: HazardPointer,
        ptr: MarkedCntObjPtr<T>,
    },
    ProtectedByRefCount {
        ptr: MarkedCntObjPtr<T>,
        has_cnt: bool,
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
            | AcquiredPtrHP::ProtectedByRefCount { ptr, .. }
            | AcquiredPtrHP::Unprotected { ptr } => ptr,
        }
    }

    #[inline(always)]
    unsafe fn deref_counted_ptr_mut(&mut self) -> &mut MarkedCntObjPtr<T> {
        match self {
            AcquiredPtrHP::ProtectedIndependently { ptr, .. }
            | AcquiredPtrHP::ProtectedByRefCount { ptr, .. }
            | AcquiredPtrHP::Unprotected { ptr } => ptr,
        }
    }

    #[inline(always)]
    fn as_counted_ptr(&self) -> MarkedCntObjPtr<T> {
        match self {
            AcquiredPtrHP::ProtectedIndependently { ptr, .. }
            | AcquiredPtrHP::ProtectedByRefCount { ptr, .. }
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
        match self {
            AcquiredPtrHP::ProtectedIndependently { ptr, .. }
            | AcquiredPtrHP::ProtectedByRefCount { ptr, .. }
            | AcquiredPtrHP::Unprotected { ptr } => ptr.is_null(),
        }
    }

    #[inline(always)]
    fn is_protected(&self) -> bool {
        match self {
            AcquiredPtrHP::ProtectedIndependently { .. } => true,
            AcquiredPtrHP::ProtectedByRefCount { has_cnt, .. } => *has_cnt,
            AcquiredPtrHP::Unprotected { .. } => false,
        }
    }

    #[inline(always)]
    fn clear_protection(&mut self) {
        match self {
            AcquiredPtrHP::ProtectedIndependently { hazptr, .. } => hazptr.reset_protection(),
            AcquiredPtrHP::ProtectedByRefCount { has_cnt, ptr } => {
                if *has_cnt {
                    unsafe { GuardHP::handle().decrement_ref_cnt(ptr.unmarked()) };
                    *has_cnt = false;
                }
            }
            AcquiredPtrHP::Unprotected { .. } => {}
        }
    }

    #[inline(always)]
    fn swap(p1: &mut Self, p2: &mut Self) {
        core::mem::swap(p1, p2);
    }

    #[inline(always)]
    fn eq(&self, other: &Self) -> bool {
        self.as_counted_ptr() == other.as_counted_ptr()
    }
}

impl AcquireRetire for GuardHP {
    type AcquiredPtr<T> = AcquiredPtrHP<T>;

    #[inline(always)]
    fn handle() -> Self {
        GuardHP {
            thread: unsafe { &*DEFAULT_THREAD.with(|t| t.as_ptr()) },
        }
    }

    #[inline(always)]
    unsafe fn unprotected() -> Self {
        Self::handle()
    }

    #[inline(always)]
    fn create_object<T>(&self, obj: T) -> *mut crate::CountedObject<T> {
        let obj = CountedObject::new(obj);
        Box::into_raw(Box::new(obj))
    }

    #[inline(always)]
    fn acquire<T>(&self, link: &atomic::Atomic<MarkedCntObjPtr<T>>) -> Self::AcquiredPtr<T> {
        let hazptr = self.acquire_shield();
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
        let hazptr = self.acquire_shield();
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
        let hazptr = self.acquire_shield();
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
    fn protect_snapshot_with<T>(
        &self,
        link: &atomic::Atomic<MarkedCntObjPtr<T>>,
        dst: &mut Self::AcquiredPtr<T>,
    ) {
        match dst {
            AcquiredPtrHP::ProtectedIndependently { hazptr, ptr } => {
                *ptr = link.load(Ordering::Relaxed);
                loop {
                    hazptr.protect_raw(ptr.unmarked());
                    membarrier::light();
                    let new_ptr = link.load(Ordering::Acquire);
                    if *ptr == new_ptr {
                        break;
                    }
                    *ptr = new_ptr;
                }
            }
            AcquiredPtrHP::ProtectedByRefCount { .. } | AcquiredPtrHP::Unprotected { .. } => {
                *dst = self.protect_snapshot(link)
            }
        }
    }

    #[inline(always)]
    fn reserve_snapshot<T>(&self, ptr: MarkedCntObjPtr<T>) -> Self::AcquiredPtr<T> {
        if ptr.is_null() {
            return AcquiredPtrHP::Unprotected {
                ptr: MarkedCntObjPtr::null(),
            };
        }
        assert!(unsafe { self.increment_ref_cnt(ptr.unmarked()) });
        AcquiredPtrHP::ProtectedByRefCount { ptr, has_cnt: true }
    }

    #[inline(always)]
    unsafe fn delete_object<T>(&self, ptr: *mut crate::CountedObject<T>) {
        drop(Box::from_raw(ptr));
    }

    #[inline(always)]
    unsafe fn retire<T>(&self, ptr: *mut crate::CountedObject<T>, ret_type: crate::RetireType) {
        let marked = MarkedCntObjPtr::new(ptr);
        self.thread().defer(marked.unmarked(), move || {
            let inner_guard = Self::unprotected();
            inner_guard.eject(ptr, ret_type);
        });
    }
}
