use std::{marker::PhantomData, ptr};

use crate::internal::{CountedPtr, MarkedPtr, AcquireRetire};

pub struct RcPtr<T, S>
where
    S: AcquireRetire<T>,
{
    ptr: CountedPtr<T>,
    _marker: PhantomData<S>,
}

impl<T, S> RcPtr<T, S>
where
    S: AcquireRetire<T>,
{
    pub(crate) fn new() -> Self {
        Self {
            ptr: MarkedPtr::new(ptr::null_mut()),
            _marker: PhantomData
        }
    }

    
}
