mod domain;
mod hazard;
mod retire;
mod thread;

pub use hazard::HazardPointer;

use std::thread_local;

use domain::Domain;
pub use thread::Thread;

pub static DEFAULT_DOMAIN: Domain = Domain::new();

pub struct ThreadPtr {
    thread: *const Thread,
}

impl ThreadPtr {
    fn new() -> Self {
        let ptr = Box::into_raw(Box::new(Thread::new(&DEFAULT_DOMAIN)));
        Self {
            thread: ptr.cast_const(),
        }
    }

    pub fn as_ptr(&self) -> *const Thread {
        self.thread
    }

    pub fn deref(&self) -> &Thread {
        unsafe { &*self.thread }
    }
}

impl Drop for ThreadPtr {
    fn drop(&mut self) {
        drop(unsafe { Box::from_raw(self.thread.cast_mut()) })
    }
}

thread_local! {
    pub static DEFAULT_THREAD: ThreadPtr = ThreadPtr::new();
}
