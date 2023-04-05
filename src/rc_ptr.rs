use std::{marker::PhantomData, ptr};

use crate::internal::{CountedObjPtr, MarkedPtr, AcquireRetire};

pub struct RcPtr<T, S>
where
    S: AcquireRetire<T>,
{
    ptr: CountedObjPtr<T>,
    _marker: PhantomData<S>,
}

impl<T, S> RcPtr<T, S>
where
    S: AcquireRetire<T>,
{
    pub(crate) fn new() -> Self {
        Self {
            ptr: MarkedPtr::null(),
            _marker: PhantomData
        }
    }

    
}
