use core::sync::atomic::{AtomicPtr, Ordering};
use core::{mem, ptr};

use super::domain::Domain;
use super::hazard::ThreadRecord;
use super::retire::Retired;

pub struct Thread<'domain> {
    pub(crate) domain: &'domain Domain,
    pub(crate) hazards: &'domain ThreadRecord,
    /// available slots of hazard array
    pub(crate) available_indices: Vec<usize>,
    pub(crate) retired: Vec<Retired>,
    pub(crate) count: usize,
}

impl<'domain> Thread<'domain> {
    pub fn new(domain: &'domain Domain) -> Self {
        let (thread, available_indices) = domain.threads.acquire();
        Self {
            domain,
            hazards: thread,
            available_indices,
            retired: Vec::new(),
            count: 0,
        }
    }
}

// stuff related to reclamation
impl<'domain> Thread<'domain> {
    const COUNTS_BETWEEN_FLUSH: usize = 64;
    const COUNTS_BETWEEN_COLLECT: usize = 128;

    fn flush_retireds(&mut self) {
        self.domain
            .num_garbages
            .fetch_add(self.retired.len(), Ordering::AcqRel);
        self.domain.retireds.push(mem::take(&mut self.retired))
    }

    // NOTE: T: Send not required because we reclaim only locally.
    #[inline]
    pub unsafe fn retire<T>(&mut self, ptr: *mut T) {
        self.defer(ptr as *mut _, move || unsafe { drop(Box::from_raw(ptr)) });
    }

    #[inline]
    pub unsafe fn defer<T, F>(&mut self, ptr: *mut T, f: F)
    where
        F: FnOnce(),
    {
        self.retired.push(Retired::new(ptr as *mut _, f));
        let count = self.count.wrapping_add(1);
        self.count = count;
        if count % Self::COUNTS_BETWEEN_FLUSH == 0 {
            self.flush_retireds();
        }
        // TODO: collecting right after pushing is kinda weird
        if count % Self::COUNTS_BETWEEN_COLLECT == 0 {
            self.do_reclamation();
        }
    }

    #[inline]
    pub(crate) fn do_reclamation(&mut self) {
        let retireds = self.domain.retireds.pop_all();
        let retireds_len = retireds.len();
        if retireds.is_empty() {
            return;
        }

        membarrier::heavy();

        let guarded_ptrs = self.domain.collect_guarded_ptrs(self);
        let not_freed: Vec<Retired> = retireds
            .into_iter()
            .filter_map(|element| {
                if guarded_ptrs.contains(&element.ptr) {
                    Some(element)
                } else {
                    unsafe { element.call() };
                    None
                }
            })
            .collect();
        self.domain
            .num_garbages
            .fetch_sub(retireds_len - not_freed.len(), Ordering::AcqRel);
        self.domain.retireds.push(not_freed);
    }
}

// stuff related to hazards
impl<'domain> Thread<'domain> {
    /// acquire hazard slot
    pub(crate) fn acquire(&mut self) -> usize {
        if let Some(idx) = self.available_indices.pop() {
            idx
        } else {
            self.grow_array();
            self.acquire()
        }
    }

    fn grow_array(&mut self) {
        let array_ptr = self.hazards.hazptrs.load(Ordering::Relaxed);
        let array = unsafe { &*array_ptr };
        let size = array.len();
        let new_size = size * 2;
        let mut new_array = Box::new(Vec::with_capacity(new_size));
        for i in 0..size {
            new_array.push(AtomicPtr::new(array[i].load(Ordering::Relaxed)));
        }
        for _ in size..new_size {
            new_array.push(AtomicPtr::new(ptr::null_mut()));
        }
        self.hazards
            .hazptrs
            .store(Box::into_raw(new_array), Ordering::Release);
        unsafe { self.retire(array_ptr) };
        self.available_indices.extend(size..new_size)
    }

    /// release hazard slot
    pub(crate) fn release(&mut self, idx: usize) {
        self.available_indices.push(idx);
    }
}

impl<'domain> Drop for Thread<'domain> {
    fn drop(&mut self) {
        self.flush_retireds();
        membarrier::heavy();
        assert!(self.retired.is_empty());
        // WARNING: Dropping HazardPointer touches available_indices. So available_indices MUST be
        // dropped after hps. For the same reason, Thread::drop MUST NOT acquire HazardPointer.
        self.available_indices.clear();
        self.domain.threads.release(self.hazards);
    }
}

impl core::fmt::Debug for Thread<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Thread")
            .field("domain", &(&self.domain as *const _))
            .field("hazards", &(&self.hazards as *const _))
            .field("available_indices", &self.available_indices.as_ptr())
            .field("retired", &format!("[...; {}]", self.retired.len()))
            .field("count", &self.count)
            .finish()
    }
}
