use std::{
    marker::PhantomData,
    mem::{self, forget, replace},
    sync::atomic::AtomicUsize,
};

use atomic::{Atomic, Ordering};
use static_assertions::const_assert;

use crate::{internal::{Acquired, Guard, Tagged, TaggedCnt}, Rc};

/// A result of unsuccessful `compare_exchange`.
///
/// It returns the ownership of [`Weak`] pointer which was given as a parameter.
pub struct CompareExchangeErrorWeak<T, P> {
    /// The `desired` pointer which was given as a parameter of `compare_exchange`.
    pub desired: P,
    /// The actual pointer value inside the atomic pointer.
    pub actual: TaggedCnt<T>,
}

pub struct AtomicWeak<T, G>
where
    G: Guard,
{
    link: Atomic<TaggedCnt<T>>,
    _marker: PhantomData<G>,
}

unsafe impl<T, G: Guard> Send for AtomicWeak<T, G> {}
unsafe impl<T, G: Guard> Sync for AtomicWeak<T, G> {}

// Ensure that TaggedPtr<T> is 8-byte long,
// so that lock-free atomic operations are possible.
const_assert!(Atomic::<TaggedCnt<u8>>::is_lock_free());
const_assert!(mem::size_of::<TaggedCnt<u8>>() == mem::size_of::<usize>());
const_assert!(mem::size_of::<Atomic<TaggedCnt<u8>>>() == mem::size_of::<AtomicUsize>());

impl<T, G> AtomicWeak<T, G>
where
    G: Guard,
{
    #[inline(always)]
    pub fn null() -> Self {
        Self {
            link: Atomic::new(Tagged::null()),
            _marker: PhantomData,
        }
    }

    /// Swap the currently stored shared pointer with the given shared pointer.
    /// This operation is thread-safe.
    /// (It is equivalent to `exchange` from the original implementation.)
    #[inline(always)]
    pub fn swap(&self, new: Weak<T, G>, order: Ordering, _: &G) -> Weak<T, G> {
        let new_ptr = new.into_ptr();
        Weak::new_without_incr(self.link.swap(new_ptr, order))
    }

    /// Atomically compares the underlying pointer with expected, and if they refer to
    /// the same managed object, replaces the current pointer with a copy of desired
    /// (incrementing its reference count) and returns true. Otherwise, returns false.
    #[inline(always)]
    pub fn compare_exchange<'g, P>(
        &self,
        expected: TaggedCnt<T>,
        desired: P,
        success: Ordering,
        failure: Ordering,
        _: &'g G,
    ) -> Result<Weak<T, G>, CompareExchangeErrorWeak<T, P>>
    where
        P: WeakPtr<T, G>,
    {
        match self
            .link
            .compare_exchange(expected, desired.as_ptr(), success, failure)
        {
            Ok(_) => {
                let weak = Weak::new_without_incr(expected);
                // Here, `into_weak_count` increment the reference count of `desired` only if
                // `desired` is `Snapshot` or its variants.
                //
                // If `desired` is `Rc`, semantically the ownership of the reference count from
                // `desired` is moved to `self`. Because of this reason, we must skip decrementing
                // the reference count of `desired`.
                desired.into_weak_count();
                Ok(weak)
            }
            Err(e) => Err(CompareExchangeErrorWeak { desired, actual: e }),
        }
    }

    #[inline(always)]
    pub fn fetch_or<'g>(&self, tag: usize, order: Ordering, _: &'g G) -> TaggedCnt<T> {
        // HACK: The size and alignment of `Atomic<TaggedCnt<T>>` will be same with `AtomicUsize`.
        // The equality of the sizes is checked by `const_assert!`.
        let link = unsafe { &*(&self.link as *const _ as *const AtomicUsize) };
        let prev = link.fetch_or(tag, order);
        TaggedCnt::new(prev as *mut _)
    }
}

impl<T, G> Drop for AtomicWeak<T, G>
where
    G: Guard,
{
    #[inline(always)]
    fn drop(&mut self) {
        let ptr = self.link.load(Ordering::SeqCst);
        unsafe {
            if let Some(cnt) = ptr.untagged().as_mut() {
                let guard = G::without_epoch();
                guard.delayed_decrement_weak_cnt(cnt);
            }
        }
    }
}

impl<T, G> Default for AtomicWeak<T, G>
where
    G: Guard,
{
    #[inline(always)]
    fn default() -> Self {
        Self::null()
    }
}

pub struct Weak<T, G>
where
    G: Guard,
{
    ptr: TaggedCnt<T>,
    _marker: PhantomData<G>,
}

impl<T, G> Weak<T, G>
where
    G: Guard,
{
    #[inline(always)]
    pub fn null() -> Self {
        Self::new_without_incr(TaggedCnt::null())
    }

    #[inline(always)]
    pub(crate) fn new_without_incr(ptr: TaggedCnt<T>) -> Self {
        Self {
            ptr,
            _marker: PhantomData,
        }
    }

    #[inline(always)]
    pub fn from_weak_snapshot<'g>(ptr: &WeakSnapshot<T, G>, guard: &'g G) -> Self {
        unsafe {
            if let Some(cnt) = ptr.as_ptr().untagged().as_ref() {
                if guard.increment_weak_cnt(cnt) {
                    return Self {
                        ptr: ptr.as_ptr(),
                        _marker: PhantomData,
                    };
                }
            }
        }
        Self::null()
    }

    #[inline(always)]
    pub fn clone(&self, guard: &G) -> Self {
        let weak = Self {
            ptr: self.ptr,
            _marker: PhantomData,
        };
        unsafe {
            if let Some(cnt) = weak.ptr.untagged().as_ref() {
                guard.increment_weak_cnt(cnt);
            }
        }
        weak
    }

    #[inline]
    pub fn finalize(self, guard: &G) {
        unsafe {
            if let Some(cnt) = self.ptr.untagged().as_mut() {
                guard.delayed_decrement_weak_cnt(cnt);
            }
        }
        // Prevent recursive finalizing.
        forget(self);
    }

    #[inline(always)]
    pub fn is_null(&self) -> bool {
        self.ptr.is_null()
    }

    #[inline]
    pub fn upgrade(&self) -> Rc<T, G> {
        todo!()
    }

    #[inline(always)]
    pub fn ref_count(&self) -> u32 {
        unsafe { self.ptr.deref().ref_count() }
    }

    #[inline(always)]
    pub fn weak_count(&self) -> u32 {
        unsafe { self.ptr.deref().weak_count() }
    }

    #[inline(always)]
    pub fn tag(&self) -> usize {
        self.ptr.tag()
    }

    #[inline(always)]
    pub fn untagged(mut self) -> Self {
        self.ptr = TaggedCnt::new(self.ptr.untagged());
        self
    }

    #[inline(always)]
    pub fn with_tag(mut self, tag: usize) -> Self {
        self.ptr.set_tag(tag);
        self
    }

    pub(crate) fn into_ptr(self) -> TaggedCnt<T> {
        let new_ptr = self.as_ptr();
        // Skip decrementing the ref count.
        forget(self);
        new_ptr
    }
}

impl<T, G> Drop for Weak<T, G>
where
    G: Guard,
{
    #[inline(always)]
    fn drop(&mut self) {
        if !self.is_null() {
            replace(self, Weak::null()).finalize(&G::new());
        }
    }
}

impl<T, G> PartialEq for Weak<T, G>
where
    G: Guard,
{
    #[inline(always)]
    fn eq(&self, other: &Self) -> bool {
        self.ptr == other.ptr
    }
}

pub struct WeakSnapshot<T, G>
where
    G: Guard,
{
    // Hint: `G::Acquired` is usually a wrapper struct containing `TaggedCnt`.
    acquired: G::Acquired<T>,
}

impl<T, G> WeakSnapshot<T, G>
where
    G: Guard,
{
    #[inline(always)]
    pub fn new() -> Self {
        Self {
            acquired: <G as Guard>::Acquired::null(),
        }
    }

    #[inline]
    pub fn load(&mut self, from: &AtomicWeak<T, G>, guard: &G) {
        self.acquired = guard.protect_snapshot(&from.link);
    }

    /// # Safety
    /// TODO
    #[inline(always)]
    pub unsafe fn deref<'g>(&self) -> &'g T {
        self.acquired.ptr().deref().data()
    }

    /// # Safety
    /// TODO
    #[inline(always)]
    pub unsafe fn deref_mut<'g>(&mut self) -> &'g mut T {
        self.acquired.ptr_mut().deref_mut().data_mut()
    }

    #[inline(always)]
    pub unsafe fn as_ref<'g>(&self) -> Option<&'g T> {
        if self.is_null() {
            None
        } else {
            Some(unsafe { self.deref() })
        }
    }

    #[inline(always)]
    pub fn is_null(&self) -> bool {
        self.acquired.is_null()
    }

    #[inline(always)]
    pub fn tag(&self) -> usize {
        self.as_ptr().tag()
    }

    #[inline(always)]
    pub fn untagged(mut self) -> Self {
        self.acquired.ptr_mut().set_tag(0);
        self
    }

    pub fn set_tag(&mut self, tag: usize) {
        self.acquired.ptr_mut().set_tag(tag);
    }

    #[inline]
    pub fn with_tag<'s>(&'s self, tag: usize) -> TaggedWeakSnapshot<'s, T, G> {
        TaggedWeakSnapshot { inner: self, tag }
    }

    #[inline(always)]
    pub fn as_usize(&self) -> usize {
        self.as_ptr().as_usize()
    }
}

impl<T, G> Drop for WeakSnapshot<T, G>
where
    G: Guard,
{
    #[inline(always)]
    fn drop(&mut self) {
        self.acquired.clear_protection();
    }
}

impl<T, G> PartialEq for WeakSnapshot<T, G>
where
    G: Guard,
{
    #[inline(always)]
    fn eq(&self, other: &Self) -> bool {
        self.acquired.eq(&other.acquired)
    }
}

/// A reference of a [`WeakSnapshot`] with a overwriting tag value.
pub struct TaggedWeakSnapshot<'s, T, G>
where
    G: Guard,
{
    pub(crate) inner: &'s WeakSnapshot<T, G>,
    pub(crate) tag: usize,
}

pub trait WeakPtr<T, G> {
    fn as_ptr(&self) -> TaggedCnt<T>;

    /// Consumes the aquired pointer, incrementing the reference count if we didn't increment
    /// it before.
    ///
    /// Semantically, it is equivalent to giving ownership of a reference count outside the
    /// environment.
    ///
    /// For example, we do nothing but forget its ownership if the pointer is [`Weak`],
    /// but increment the reference count if the pointer is [`WeakSnapshot`].
    fn into_weak_count(self);
}

impl<T, G> WeakPtr<T, G> for Weak<T, G>
where
    G: Guard,
{
    fn as_ptr(&self) -> TaggedCnt<T> {
        self.ptr
    }

    fn into_weak_count(self) {
        // As we have a reference count already, we don't have to do anything, but
        // prevent calling a destructor which decrements it.
        forget(self);
    }
}

impl<T, G> WeakPtr<T, G> for WeakSnapshot<T, G>
where
    G: Guard,
{
    fn as_ptr(&self) -> TaggedCnt<T> {
        self.acquired.as_ptr()
    }

    fn into_weak_count(self) {
        if let Some(cnt) = unsafe { self.as_ptr().untagged().as_ref() } {
            cnt.add_ref();
        }
    }
}

impl<T, G> WeakPtr<T, G> for &WeakSnapshot<T, G>
where
    G: Guard,
{
    fn as_ptr(&self) -> TaggedCnt<T> {
        self.acquired.as_ptr()
    }

    fn into_weak_count(self) {
        if let Some(cnt) = unsafe { self.as_ptr().untagged().as_ref() } {
            cnt.add_ref();
        }
    }
}

impl<'s, T, G> WeakPtr<T, G> for TaggedWeakSnapshot<'s, T, G>
where
    G: Guard,
{
    fn as_ptr(&self) -> TaggedCnt<T> {
        self.inner.acquired.as_ptr().with_tag(self.tag)
    }

    fn into_weak_count(self) {
        if let Some(cnt) = unsafe { self.as_ptr().untagged().as_ref() } {
            cnt.add_ref();
        }
    }
}
