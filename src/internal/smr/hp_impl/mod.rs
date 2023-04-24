mod domain;
mod hazard;
mod retire;
mod thread;

pub use hazard::HazardPointer;

use core::cell::RefCell;
use std::thread_local;

use domain::Domain;
pub use thread::Thread;

pub static DEFAULT_DOMAIN: Domain = Domain::new();

// NOTE: MUST NOT take raw pointer to TLS. They randomly move???
thread_local! {
    pub static DEFAULT_THREAD: RefCell<Box<Thread<'static>>> = RefCell::new(Box::new(Thread::new(&DEFAULT_DOMAIN)));
}

#[inline]
pub unsafe fn defer<T, F>(ptr: *mut T, f: F)
where
    F: FnOnce(),
{
    DEFAULT_THREAD.with(|t| t.borrow().defer(ptr, f))
}
