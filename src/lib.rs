mod atomic_rc_ptr;
mod internal;
mod rc_ptr;
mod snapshot_ptr;

pub use atomic_rc_ptr::AtomicRcPtr;
pub use internal::GuardEBR;
pub use rc_ptr::RcPtr;
pub use snapshot_ptr::SnapshotPtr;

/// AtomicRcPtr using EBR
pub type AtomicRcPtrEBR<T> = AtomicRcPtr<T, GuardEBR<T>>;
/// RcPtr using EBR
pub type RcPtrEBR<T> = RcPtr<T, GuardEBR<T>>;
/// SnapshotPtr using EBR
pub type SnapshotPtrEBR<T> = SnapshotPtr<T, GuardEBR<T>>;
