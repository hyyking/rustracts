use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};

/// Arc pointer wrapper than can only emit LockWeak references that can upgrade to the inner Arc
/// only if the only lock flag allows it to. This allows safe consumption of the Arc and return the
/// inner value of it with the knowledge that every following upgrades will fail.
/// Lockable Arc pointer for consumption of the inner value.
pub struct LockArc<T> {
    inner: Arc<T>,
    lock: Arc<AtomicBool>,
}

impl<T> LockArc<T> {
    /// Build a LockArc for any value
    pub fn new(val: T) -> Self {
        Self {
            inner: Arc::new(val),
            lock: Arc::new(AtomicBool::new(false)),
        }
    }

    #[inline]
    /// Create a LockWeak reference to the underlying Arc. To prevent following upgrades from the weak pointer
    /// call `lock_arc()` or `consumme()` on the LockArc.
    pub fn downgrade(other: &LockArc<T>) -> LockWeak<T> {
        LockWeak::from(other)
    }

    /// Lock the LockArc and all it's LockWeak references returning the underlying Arc pointer.
    pub fn lock_arc(self) -> Arc<T> {
        while self.lock.compare_and_swap(false, true, Ordering::Acquire) {}
        self.inner
    }

    #[inline]
    /// Locks the LockArc and all it's LockWeak references, then consummes the Arc pointer returning the
    /// underlying value.
    pub fn consumme(self) -> T {
        // Safe because the lock assures there is only one Arc pointer after the lock
        match Arc::try_unwrap(self.lock_arc()) {
            Ok(inner) => inner,
            Err(_) => unreachable!(),
        }
    }
}

impl<T> std::ops::Deref for LockArc<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

/// Weak pointer that can be locked so it can no longer upgrade successfully although the value is still present. This prevents adding to the reference
/// count for constantly upgrading loops that read an Arc pointer that will be consummed at some
/// point.
pub struct LockWeak<T> {
    inner: Weak<T>,
    lock: Arc<AtomicBool>,
}

impl<T> LockWeak<T> {
    /// Upgrade the Weak pointer into the underlying Arc pointer adding to the reference count.
    /// Once the LockWeak has been locked this function will always return `None`. Once this
    /// function returns None once it will never return the underlying Arc because it points to
    /// nothing or has been locked.
    pub fn upgrade(&self) -> Option<Arc<T>> {
        if self.lock.load(Ordering::Acquire) {
            None
        } else {
            self.inner.upgrade()
        }
    }
}

impl<T> From<&LockArc<T>> for LockWeak<T> {
    fn from(arc: &LockArc<T>) -> Self {
        Self {
            inner: Arc::downgrade(&arc.inner),
            lock: arc.lock.clone(),
        }
    }
}
