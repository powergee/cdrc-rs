use std::{marker::PhantomData, ptr, mem};

use crate::internal::{MarkedPtr, AcquireRetire};

pub struct SnapshotPtr<T, S>
where
    S: AcquireRetire<T>,
{
    acquired: MarkedPtr<T>,
    _marker: PhantomData<S>,
}

impl<T, S> SnapshotPtr<T, S>
where
    S: AcquireRetire<T>,
{
    pub(crate) fn new() -> Self {
        Self {
            acquired: MarkedPtr::new(ptr::null_mut()),
            _marker: PhantomData,
        }
    }

    pub fn deref(&self) -> &T {
        unsafe { self.acquired.deref() }
    }

    pub fn deref_mut(&self) -> &mut T {
        unsafe { self.acquired.deref_mut() }
    }
    
    pub fn is_null(&self) -> bool {
        self.acquired.is_null()
    }

    pub fn swap(lhs: &mut Self, rhs: &mut Self) {
        mem::swap(lhs, rhs);
    }

    pub fn clear(&self) {
        let ptr = unsafe { self.acquired.deref_mut() };
        
    }
}

impl<T, S> PartialEq for SnapshotPtr<T, S>
where
    S: AcquireRetire<T>,
{
    fn eq(&self, other: &Self) -> bool {
        self.acquired == other.acquired
    }
}
