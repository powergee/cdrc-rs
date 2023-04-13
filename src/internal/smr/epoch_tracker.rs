use std::cell::Cell;

use atomic::{Atomic, Ordering};
use crossbeam_utils::CachePadded;

pub(crate) type Epoch = u64;

const NO_EPOCH: Epoch = Epoch::MAX;

/// A minimal epoch tracker which is used for EBR, IBR, and etc.
/// Usually they use `EpochTracker` as a singleton manner.
pub(crate) struct EpochTracker {
    global_epoch: CachePadded<Atomic<Epoch>>,
    local_epoch: Vec<CachePadded<Atomic<Epoch>>>,
    in_critical: Vec<CachePadded<Cell<bool>>>,
}

impl EpochTracker {
    pub const fn new() -> Self {
        Self {
            global_epoch: CachePadded::new(Atomic::new(0)),
            // The capacity of `local_epoch` and `in_critical` must
            // be set in `set_max_threads` if necessary.
            local_epoch: Vec::new(),
            in_critical: Vec::new(),
        }
    }

    pub fn max_threads(&self) -> usize {
        self.local_epoch.len()
    }

    pub fn set_max_threads(&mut self, threads: usize) {
        self.local_epoch
            .resize_with(threads, || CachePadded::new(Atomic::new(NO_EPOCH)));
        self.in_critical
            .resize(threads, CachePadded::new(Cell::new(false)));
    }

    pub fn current_epoch(&self) -> Epoch {
        self.global_epoch.load(Ordering::SeqCst)
    }

    pub fn advance_global_epoch(&self) {
        self.global_epoch.fetch_add(1, Ordering::SeqCst);
    }

    pub fn min_announced_epoch(&self) -> Epoch {
        self.local_epoch
            .iter()
            .fold(NO_EPOCH, |acc, epoch| acc.min(epoch.load(Ordering::SeqCst)))
    }

    pub fn begin_critical_section(&self, id: usize) {
        assert!(!self.in_critical_section(id));

        let current = self.global_epoch.load(Ordering::Acquire);
        self.local_epoch[id].swap(current, Ordering::SeqCst);
        self.in_critical[id].set(true);
    }

    pub fn end_critical_section(&self, id: usize) {
        assert!(self.in_critical_section(id));
        assert!(self.local_epoch[id].load(Ordering::SeqCst) != NO_EPOCH);

        self.local_epoch[id].store(NO_EPOCH, Ordering::Release);
        self.in_critical[id].set(false);
    }

    pub fn in_critical_section(&self, id: usize) -> bool {
        self.in_critical[id].get()
    }
}
