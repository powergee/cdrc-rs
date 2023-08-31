#![feature(associated_type_bounds)]
mod internal;
mod pointers;

pub use internal::{Acquired, Counted, EjectAction, Guard, GuardEBR, RetireType, TaggedCnt};
pub use pointers::*;

/// AtomicRc using EBR
pub type AtomicRcEBR<T> = AtomicRc<T, GuardEBR>;
/// Rc using EBR
pub type RcEBR<T> = Rc<T, GuardEBR>;
/// Snapshot using EBR
pub type SnapshotEBR<T> = Snapshot<T, GuardEBR>;
