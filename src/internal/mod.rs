mod smr;
mod smr_common;
mod utils;

pub use smr::GuardEBR;
pub use smr_common::{AcquireRetire, AcquiredPtr, RetireType};
pub use utils::{CountedObject, EjectAction};

pub(crate) use utils::*;
