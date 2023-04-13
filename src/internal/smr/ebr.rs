use std::cell::{Cell, RefCell};
use std::mem;
use std::ops::Deref;
use std::sync::atomic::AtomicUsize;

use atomic::Ordering;
use crossbeam_utils::CachePadded;

use crate::internal::smr_common::Handle;
use crate::internal::utils::CountedObject;
use crate::internal::{AcquireRetire, AcquiredPtr, MarkedCntObjPtr, RetireType};
use crate::EjectAction;

use super::epoch_tracker::{Epoch, EpochTracker};

const EPOCH_FREQUENCY: usize = 10;
const EJECT_DELAY: usize = 2;
const UNPROTECTED_TID: usize = usize::MAX;

struct Record {
    ptr: *mut CountedObject<u8>,
    deleter: unsafe fn(*mut CountedObject<u8>),
    retire_ts: Option<Epoch>,
    ret_type: Option<RetireType>,
}

impl Record {
    fn new<T>(ptr: *mut CountedObject<T>) -> Self {
        Self {
            ptr: ptr as *mut _,
            deleter: delete::<T>,
            retire_ts: None,
            ret_type: None,
        }
    }

    fn as_dispose(&self) -> Self {
        Self {
            ret_type: Some(RetireType::Dispose),
            ..*self
        }
    }

    fn as_decrement_strong(&self) -> Self {
        Self {
            ret_type: Some(RetireType::DecrementStrongCount),
            ..*self
        }
    }

    fn as_decrement_weak(&self) -> Self {
        Self {
            ret_type: Some(RetireType::DecrementWeakCount),
            ..*self
        }
    }

    fn with_ts(&self, ts: Epoch) -> Self {
        Self {
            retire_ts: Some(ts),
            ..*self
        }
    }
}

struct BaseEBR {
    tracker: EpochTracker,
    registered_count: AtomicUsize,

    // Local flags to prevent reentrancy while destructing
    in_progress: Vec<CachePadded<Cell<bool>>>,
    // Thread-local lists of pending deferred destructs
    deferred: Vec<CachePadded<RefCell<Vec<Record>>>>,
    // Amortized work to pay for ejecting deferred destructs
    eject_work: Vec<CachePadded<Cell<usize>>>,
    // Amortized work to pay for incrementing the epoch
    epoch_work: Vec<CachePadded<Cell<usize>>>,
}

impl Drop for BaseEBR {
    // Perform any remaining deferred destruction. Need to be very careful
    // about additional objects being queued for deferred destruction by
    // an object that was just destructed.
    fn drop(&mut self) {
        self.in_progress.iter_mut().for_each(|x| x.set(true));

        // Loop because the destruction of one object could trigger the deferred
        // destruction of another object (possibly even in another thread), and
        // so on recursively.
        while self.deferred.iter().any(|v| !v.borrow().is_empty()) {
            // Move all of the contents from the deferred destruction lists
            // into a single local list. We don't want to just iterate the
            // deferred lists because a destruction may trigger another
            // deferred destruction to be added to one of the lists, which
            // would invalidate its iterators
            let jobs = self
                .deferred
                .iter()
                .flat_map(|deferred| deferred.take())
                .collect::<Vec<_>>();

            // Perform all of the pending deferred ejects
            for job in jobs {
                unsafe { self.eject(0, job) };
            }
        }
    }
}

impl BaseEBR {
    fn num_threads(&self) -> usize {
        self.in_progress.len()
    }

    unsafe fn retire(&self, tid: usize, record: Record) {
        if tid == UNPROTECTED_TID {
            self.eject(tid, record);
            return;
        }

        self.deferred[tid]
            .deref()
            .borrow_mut()
            .push(record.with_ts(self.tracker.current_epoch()));
        self.work_toward_ejects(tid, 1);
    }

    unsafe fn dispose(&self, record: Record) {
        assert!((*record.ptr).use_count() == 0);
        (*record.ptr).dispose();
        if (*record.ptr).release_weak_refs(1) {
            self.destroy(record);
        }
    }

    unsafe fn destroy(&self, record: Record) {
        assert!((*record.ptr).use_count() == 0);
        (record.deleter)(record.ptr);
    }

    /// Perform an eject action. This can correspond to any action that
    /// should be delayed until the ptr is no longer protected
    unsafe fn eject(&self, tid: usize, record: Record) {
        assert!(!record.ptr.is_null());

        match record.ret_type.as_ref().unwrap() {
            RetireType::DecrementStrongCount => self.decrement_ref_cnt(tid, record),
            RetireType::DecrementWeakCount => self.decrement_weak_cnt(record),
            RetireType::Dispose => self.dispose(record),
        }
    }

    unsafe fn decrement_ref_cnt(&self, tid: usize, record: Record) {
        assert!(!record.ptr.is_null());
        assert!((*record.ptr).use_count() >= 1);
        let result = (*record.ptr).release_refs(1);

        match result {
            EjectAction::Nothing => {}
            EjectAction::Delay => self.retire(tid, record.as_dispose()),
            EjectAction::Destroy => self.destroy(record),
        }
    }

    unsafe fn decrement_weak_cnt(&self, record: Record) {
        assert!(!record.ptr.is_null());
        assert!((*record.ptr).weak_count() >= 1);
        if (*record.ptr).release_weak_refs(1) {
            self.destroy(record);
        }
    }

    unsafe fn delayed_decrement_ref_cnt(&self, tid: usize, record: Record) {
        assert!((*record.ptr).use_count() >= 1);
        self.retire(tid, record.as_decrement_strong());
    }

    unsafe fn delayed_decrement_weak_cnt(&self, tid: usize, record: Record) {
        assert!((*record.ptr).weak_count() >= 1);
        self.retire(tid, record.as_decrement_weak());
    }

    fn work_toward_advancing_epoch(&self, tid: usize, work: usize) {
        if tid == UNPROTECTED_TID {
            return;
        }

        self.epoch_work[tid].set(self.epoch_work[tid].get() + work);
        if self.epoch_work[tid].get() >= EPOCH_FREQUENCY * self.num_threads() {
            self.epoch_work[tid].set(0);
            self.tracker.advance_global_epoch();
        }
    }

    fn work_toward_ejects(&self, tid: usize, work: usize) {
        if tid == UNPROTECTED_TID {
            return;
        }

        self.eject_work[tid].set(self.eject_work[tid].get() + work);
        // Always attempt at least 30 ejects
        let threshold = 30.max(EJECT_DELAY * self.num_threads());

        while !self.in_progress[tid].get() && self.eject_work[tid].get() > threshold {
            self.eject_work[tid].set(0);
            if self.deferred.is_empty() {
                // nothing to collect
                break;
            }
            self.in_progress[tid].set(true);

            let min_epoch = self.tracker.min_announced_epoch();

            // Remove the deferred decrements that are successfully applied
            let removed = self.deferred[tid]
                .deref()
                .borrow_mut()
                .drain(..)
                .filter_map(|def| {
                    if def.retire_ts.unwrap() < min_epoch {
                        unsafe { self.eject(tid, def) };
                        return None;
                    }
                    Some(def)
                })
                .collect();
            *self.deferred[tid].deref().borrow_mut() = removed;

            self.in_progress[tid].set(false);
        }
    }
}

/// A singleton Epoch manager.
static mut BASE_EBR: BaseEBR = BaseEBR {
    tracker: EpochTracker::new(),
    registered_count: AtomicUsize::new(0),

    // The capacity of all thread-local vectors must
    // be set in `set_max_threads` if necessary.
    in_progress: Vec::new(),
    deferred: Vec::new(),
    eject_work: Vec::new(),
    epoch_work: Vec::new(),
};

pub struct HandleEBR {
    tid: usize,
}

impl Handle for HandleEBR {
    type Guard = GuardEBR;

    unsafe fn set_max_threads(threads: usize) {
        assert!(
            BASE_EBR.registered_count.load(Ordering::SeqCst) == 0,
            "`Handle::set_max_threads` must be called before registering"
        );
        BASE_EBR.tracker.set_max_threads(threads);
        BASE_EBR.in_progress.resize(threads, Default::default());
        BASE_EBR
            .deferred
            .resize_with(threads, || Default::default());
        BASE_EBR.eject_work.resize(threads, Default::default());
        BASE_EBR.epoch_work.resize(threads, Default::default());
    }

    unsafe fn reset_registrations() {
        BASE_EBR.registered_count.store(0, Ordering::SeqCst);
    }

    fn register() -> Self {
        let tid = unsafe { BASE_EBR.registered_count.fetch_add(1, Ordering::SeqCst) };
        assert!(tid < unsafe { BASE_EBR.tracker.max_threads() });
        Self { tid }
    }

    fn pin(&self) -> Self::Guard {
        GuardEBR::new(self.tid)
    }
}

pub struct GuardEBR {
    tid: usize,
}

impl GuardEBR {
    fn new(tid: usize) -> Self {
        let guard = Self { tid };
        guard.base().tracker.begin_critical_section(tid);
        guard
    }

    /// Assume that Global EBR is properly initialized,
    /// and return its immutable reference.
    fn base(&self) -> &'static BaseEBR {
        unsafe { &BASE_EBR }
    }
}

impl Drop for GuardEBR {
    fn drop(&mut self) {
        self.base().tracker.end_critical_section(self.tid);
    }
}

impl AcquireRetire for GuardEBR {
    type AcquiredPtr<T> = AcquiredPtrEBR<T>;

    unsafe fn unprotected<'g>() -> &'g Self {
        struct GuardWrapper(GuardEBR);
        unsafe impl Sync for GuardWrapper {}
        static UNPROTECTED: GuardWrapper = GuardWrapper(GuardEBR {
            tid: UNPROTECTED_TID,
        });
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
        self.base().decrement_ref_cnt(self.tid, Record::new(ptr));
    }

    unsafe fn decrement_weak_cnt<T>(&self, ptr: *mut CountedObject<T>) {
        self.base().decrement_weak_cnt(Record::new(ptr));
    }

    unsafe fn delayed_decrement_ref_cnt<T>(&self, ptr: *mut CountedObject<T>) {
        self.base()
            .delayed_decrement_ref_cnt(self.tid, Record::new(ptr));
    }

    unsafe fn delayed_decrement_weak_cnt<T>(&self, ptr: *mut CountedObject<T>) {
        self.base()
            .delayed_decrement_weak_cnt(self.tid, Record::new(ptr));
    }

    fn create_object<T>(&self, obj: T) -> *mut CountedObject<T> {
        self.base().work_toward_advancing_epoch(self.tid, 1);
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

unsafe fn delete<T>(obj: *mut CountedObject<u8>) {
    let obj = obj as *mut CountedObject<T>;
    unsafe { drop(Box::from_raw(obj)) };
}
