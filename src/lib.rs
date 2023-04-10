mod atomic_rc_ptr;
mod internal;
mod rc_ptr;
mod snapshot_ptr;

pub use internal::{AcquireRetire, AcquiredPtr, CountedObject, EjectAction, GuardEBR, RetireType};

pub use atomic_rc_ptr::AtomicRcPtr;
pub use rc_ptr::RcPtr;
pub use snapshot_ptr::SnapshotPtr;

/// AtomicRcPtr using EBR
pub type AtomicRcPtrEBR<T> = AtomicRcPtr<T, GuardEBR>;
/// RcPtr using EBR
pub type RcPtrEBR<'g, T> = RcPtr<'g, T, GuardEBR>;
/// SnapshotPtr using EBR
pub type SnapshotPtrEBR<'g, T> = SnapshotPtr<'g, T, GuardEBR>;
