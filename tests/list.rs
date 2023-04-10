#![feature(inherent_associated_types)]
use cdrc_rs::{AtomicRcPtr, RcPtr, AcquireRetire};

use std::cmp::Ordering::{Equal, Greater, Less};
use std::mem;

/// Some or executing the given expression.
macro_rules! some_or {
    ($e:expr, $err:expr) => {{
        match $e {
            Some(r) => r,
            None => $err,
        }
    }};
}

struct Node<K, V, Guard>
where
    Guard: AcquireRetire,
{
    /// Mark: tag(), Tag: not needed
    next: AtomicRcPtr<Self, Guard>,
    key: Option<K>,
    value: Option<V>,
}

struct List<K, V, Guard>
where
    Guard: AcquireRetire
{
    head: AtomicRcPtr<Node<K, V, Guard>, Guard>,
}

// impl<K, V, Guard> Default for List<K, V, Guard>
// where
//     K: Ord,
//     Guard: AcquireRetire
// {
//     fn default() -> Self {
//         Self::new()
//     }
// }

impl<K, V, Guard> Drop for List<K, V, Guard>
where
    Guard: AcquireRetire
{
    fn drop(&mut self) {
        let guard = &Guard::handle();
        unsafe {
            let mut curr = self.head.load_rc(guard);

            while !curr.is_null() {
                let curr_ref = curr.deref_mut();
                let next = curr_ref.next.load_rc(guard);
                curr_ref.next.store_null(guard);
                curr = next;
            }
        }
    }
}

impl<K, V, Guard> Node<K, V, Guard>
where
    Guard: AcquireRetire
{
    /// Creates a new node.
    fn new(key: K, value: V) -> Self {
        Self {
            next: AtomicRcPtr::null(),
            key: Some(key),
            value: Some(value),
        }
    }

    fn head() -> Self {
        Self {
            next: AtomicRcPtr::null(),
            key: None,
            value: None,
        }
    }
}

struct Cursor<K, V, Guard>
where
    Guard: AcquireRetire
{
    prev: RcPtr<Node<K, V, Guard>, Guard>,
    // Tag of `curr` should always be zero so when `curr` is stored in a `prev`, we don't store a
    // marked pointer and cause cleanup to fail.
    curr: RcPtr<Node<K, V, Guard>, Guard>,
}

impl<K, V, Guard> Cursor<K, V, Guard>
where
    K: Ord,
    Guard: AcquireRetire,
{
    /// Creates a cursor.
    fn new(head: &AtomicRcPtr<Node<K, V, Guard>, Guard>, guard: &Guard) -> Self {
        let prev = head.load_rc(guard);
        let curr = unsafe { prev.deref() }.next.load_rc(guard);
        Self { prev, curr }
    }

    /// Clean up a chain of logically removed nodes in each traversal.
    #[inline]
    fn find_harris(&mut self, key: &K, guard: &Guard) -> Result<bool, ()> {
        // Finding phase
        // - cursor.curr: first unmarked node w/ key >= search key (4)
        // - cursor.prev: the ref of .next in previous unmarked node (1 -> 2)
        // 1 -> 2 -x-> 3 -x-> 4 -> 5 -> ∅  (search key: 4)
        let mut prev_next = self.curr.clone(guard);
        let found = loop {
            let curr_node = some_or!(self.curr.as_ref(), break false);
            let next = curr_node.next.load_rc(guard);

            // - finding stage is done if cursor.curr advancement stops
            // - advance cursor.curr if (.next is marked) || (cursor.curr < key)
            // - stop cursor.curr if (not marked) && (cursor.curr >= key)
            // - advance cursor.prev if not marked

            if next.mark() != 0 {
                // We add a 0 tag here so that `self.curr`s tag is always 0.
                self.curr = next.with_mark(0);
                continue;
            }

            match curr_node.key.as_ref().unwrap().cmp(key) {
                Less => {
                    mem::swap(&mut self.prev, &mut self.curr);
                    self.curr = next.clone(guard);
                    prev_next = next;
                }
                Equal => break true,
                Greater => break false,
            }
        };

        // If prev and curr WERE adjacent, no need to clean up
        if prev_next == self.curr {
            return Ok(found);
        }

        // cleanup marked nodes between prev and curr
        if unsafe { self.prev.deref() }
            .next
            .compare_exchange(
                prev_next.clone(guard),
                self.curr.clone(guard),
                guard,
            ) {
                return Err(());
            }

        // defer_destroy from cursor.prev.load() to cursor.curr (exclusive)
        let mut node = prev_next;
        while !node.eq_without_tag(&self.curr) {
            let node_ref = unsafe { node.deref() };
            let next = node_ref.next.load_rc(guard);
            node_ref.next.store_null(guard);
            node = next;
        }

        Ok(found)
    }

    /// gets the value.
    #[inline]
    pub fn get(&self) -> Option<&V> {
        self.curr.as_ref().map(|n| n.value.as_ref().unwrap())
    }

    /// Inserts a value.
    #[inline]
    pub fn insert(
        &mut self,
        node: RcPtr<Node<K, V, Guard>, Guard>,
        guard: &Guard,
    ) -> Result<(), RcPtr<Node<K, V, Guard>, Guard>> {
        let curr = mem::take(&mut self.curr);
        unsafe { node.deref() }.next.store_rc_relaxed(curr.clone(guard), guard);

        if unsafe { self.prev.deref() }.next.compare_exchange(
            curr,
            node.clone(guard),
            guard,
        ) {
            self.curr = node;
            Ok(())
        } else {
            Err(node)
        }
    }

    /// removes the current node.
    #[inline]
    pub fn remove(self, guard: &Guard) -> Result<&V, ()> {
        let curr_node = unsafe { self.curr.deref() };

        let next_unmarked = curr_node.next.load_rc(guard);
        if next_unmarked.mark() != 0 {
            return Err(());
        }

        let next_marked = next_unmarked.with_mark(1);
        if curr_node.next.compare_exchange(next_unmarked, desired, guard) {
            todo!()
        }
        let next = curr_node.next.fetch_or(1, Ordering::Acquire, guard);
        if next.tag() == 1 {
            return Err(());
        }

        if self
            .prev
            .compare_exchange(self.curr, next, Ordering::Release, Ordering::Relaxed, guard)
            .is_ok()
        {
            unsafe { guard.defer_destroy(self.curr) };
        }

        Ok(&curr_node.value)
    }
}