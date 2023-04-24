use std::cell::Cell;

use atomic::Ordering;

use crate::{internal::MarkedCntObjPtr, AcquireRetire, AcquiredPtr, CountedObject};

use super::hp_impl::{defer, HazardPointer};

const SHIELD_COUNT: usize = 7;

#[derive(Default)]
pub struct GuardData {
    shield: HazardPointer<'static>,
    snapshot_shield: [HazardPointer<'static>; SHIELD_COUNT],
    snapshot_in_use: [Cell<bool>; SHIELD_COUNT],
}

impl GuardData {
    fn new() -> Self {
        let mut data = GuardData {
            shield: Default::default(),
            snapshot_shield: Default::default(),
            snapshot_in_use: Default::default(),
        };

        data.shield.reset_protection();
        for shield in &mut data.snapshot_shield {
            shield.reset_protection();
        }
        data
    }

    fn get_free_shield<'g>(&'g self) -> Option<(&'g HazardPointer<'static>, &'g Cell<bool>)> {
        for i in 0..SHIELD_COUNT {
            if !self.snapshot_in_use[i].get() {
                self.snapshot_in_use[i].set(true);
                return Some((&self.snapshot_shield[i], &self.snapshot_in_use[i]));
            }
        }
        None
    }
}

pub struct GuardHP {
    shield: Option<GuardData>,
}

pub struct AcquiredPtrHP<T> {
    hazptr: *const HazardPointer<'static>,
    ptr: MarkedCntObjPtr<T>,
    in_use: *const Cell<bool>,
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
            hazptr: core::ptr::null_mut(),
            ptr: MarkedCntObjPtr::null(),
            in_use: core::ptr::null_mut(),
        }
    }

    #[inline(always)]
    fn is_null(&self) -> bool {
        self.ptr.is_null()
    }

    #[inline(always)]
    fn is_protected(&self) -> bool {
        !self.hazptr.is_null()
    }

    #[inline(always)]
    fn clear_protection(&mut self) {
        if let Some(hazptr) = unsafe { self.hazptr.as_ref() } {
            hazptr.reset_protection();
            self.hazptr = core::ptr::null();
        }
        unsafe {
            if let Some(in_use) = self.in_use.as_ref() {
                in_use.set(false);
                self.in_use = core::ptr::null();
            }
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

impl<T> Drop for AcquiredPtrHP<T> {
    fn drop(&mut self) {
        self.clear_protection();
    }
}

impl AcquireRetire for GuardHP {
    type AcquiredPtr<T> = AcquiredPtrHP<T>;

    #[inline(always)]
    fn handle() -> Self {
        GuardHP {
            shield: Some(GuardData::new()),
        }
    }

    #[inline(always)]
    unsafe fn unprotected() -> Self {
        GuardHP { shield: None }
    }

    #[inline(always)]
    fn create_object<T>(&self, obj: T) -> *mut crate::CountedObject<T> {
        let obj = CountedObject::new(obj);
        Box::into_raw(Box::new(obj))
    }

    #[inline(always)]
    fn acquire<T>(&self, link: &atomic::Atomic<MarkedCntObjPtr<T>>) -> Self::AcquiredPtr<T> {
        assert!(self.shield.is_some());
        let data = self.shield.as_ref().unwrap();
        let mut ptr = link.load(Ordering::Relaxed);

        loop {
            data.shield.protect_raw(ptr.unmarked());
            membarrier::light();
            let new_ptr = link.load(Ordering::Acquire);
            if ptr == new_ptr {
                break;
            }
            ptr = new_ptr;
        }
        AcquiredPtrHP {
            hazptr: &data.shield,
            ptr,
            in_use: core::ptr::null_mut(),
        }
    }

    #[inline(always)]
    fn reserve<T>(&self, ptr: *mut crate::CountedObject<T>) -> Self::AcquiredPtr<T> {
        assert!(self.shield.is_some());
        let data = self.shield.as_ref().unwrap();
        let ptr = MarkedCntObjPtr::new(ptr);
        data.shield.protect_raw(ptr.unmarked());
        membarrier::light();
        AcquiredPtrHP {
            hazptr: &data.shield,
            ptr,
            in_use: core::ptr::null_mut(),
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
        assert!(self.shield.is_some());
        let data = self.shield.as_ref().unwrap();
        let mut ptr = link.load(Ordering::Relaxed);

        if let Some((hazptr, in_use)) = data.get_free_shield() {
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
                hazptr,
                ptr,
                in_use,
            }
        } else {
            loop {
                let acquired = self.acquire(link);
                if !acquired.is_null() {
                    assert!(unsafe {
                        self.increment_ref_cnt(acquired.as_counted_ptr().unmarked())
                    });
                    return AcquiredPtrHP {
                        hazptr: core::ptr::null(),
                        ptr: acquired.ptr,
                        in_use: core::ptr::null_mut(),
                    };
                } else if acquired.is_null()
                    || link.load(Ordering::Acquire).as_usize()
                        == acquired.as_counted_ptr().as_usize()
                {
                    return AcquiredPtrHP {
                        hazptr: core::ptr::null(),
                        ptr: MarkedCntObjPtr::null(),
                        in_use: core::ptr::null_mut(),
                    };
                }
            }
        }
    }

    #[inline(always)]
    fn reserve_snapshot<T>(&self, ptr: MarkedCntObjPtr<T>) -> Self::AcquiredPtr<T> {
        if ptr.is_null() {
            return AcquiredPtrHP {
                hazptr: core::ptr::null(),
                ptr: MarkedCntObjPtr::null(),
                in_use: core::ptr::null_mut(),
            };
        }
        assert!(unsafe { self.increment_ref_cnt(ptr.unmarked()) });
        AcquiredPtrHP {
            hazptr: core::ptr::null(),
            ptr,
            in_use: core::ptr::null_mut(),
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
