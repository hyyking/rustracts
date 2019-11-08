//! This crate exposes [`ParentArc<T>`](struct.ParentArc.html) which is comparable to an
//! [`Arc<T>`](https://doc.rust-lang.org/std/sync/struct.Arc.html) but "strong" references cannot
//! be cloned which allows the `ParentArc<T>` to lock its weak references and block until all
//! strong references are dropped. Once it is the only reference it can be consummed safely.
//!
//! This crate is compatible with
//! [`#![no_std]`](https://rust-embedded.github.io/book/intro/no-std.html) environnements that
//! provide an allocator.

#![no_std]
#![deny(missing_docs)]

#[cfg(not(feature = "std"))]
mod imports {
    extern crate alloc;
    pub(super) use alloc::boxed::Box;
}

#[cfg(feature = "std")]
mod imports {
    extern crate std;
    pub(super) use std::boxed::Box;
    pub(super) use std::fmt;
}

use imports::*;

use core::mem;
use core::ops;
use core::pin::Pin;
use core::ptr;
use core::ptr::NonNull;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Result Type for [`try_into_inner`]
///
/// [`try_into_inner`]: struct.ParentArc.html#method.try_into_inner
pub type TryUnwrapResult<T> = Result<T, TryUnwrapError<T>>;

/// Errors for [`TryArcResult`](type.TryUnwrapResult.html)
pub enum TryUnwrapError<T> {
    /// Would have locked the Temp references
    WouldLock(ParentArc<T>),

    /// Would have blocked becasue there is still a [`ChildArc`](struct.ChildArc.html) reference
    WouldBlock(ParentArc<T>),
}

#[cfg(feature = "std")]
impl<T> fmt::Debug for TryUnwrapError<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            TryUnwrapError::WouldLock(_) => write!(f, "WouldLock(...)"),
            TryUnwrapError::WouldBlock(_) => write!(f, "WouldBlock(...)"),
        }
    }
}

/// Owner of multiple atomically reference counted children.
///
/// The type `ParentArc<T>` allows for shared access of the inner data by multiple threads through LockWeak references.
/// Call downgrade on a `ParentArc` to create a child reference that can be upgraded into a
/// temporary reader of the inner data. This allows for the locking and the consumption of the
/// parent at any time because no strong references are held permanently.
///
/// Unlike [`Arc<T>`](https://doc.rust-lang.org/std/sync/struct.Arc.html) this structure will die
/// along with it's readers.
///
/// # Thread Safety
/// The [`LockWeak`](struct.LockWeak) can be passed around through threads safely because they do
/// not guaranty the existence of the data at upgrade time.
/// `ParentArc<T>` makes it thread safe to have multiple owned reference of the same data, but it doesn't add thread safety to its data.
pub struct ParentArc<T> {
    ptr: NonNull<Womb<T>>,
}

impl<T> ParentArc<T> {
    /// Build a new [`ParentArc`](struct.ParentArc.html)
    ///
    /// # Examples
    /// ```rust
    /// use parc::ParentArc;
    /// use std::sync::Mutex;
    /// fn main() {
    ///     let parent = ParentArc::new(Mutex::new(true));
    /// }
    /// ```
    pub fn new(data: T) -> Self {
        Self {
            ptr: Womb::as_nnptr(data),
        }
    }

    /// Constructs a new `Pin<ParentArc<T>>`. If `T` does not implement `Unpin`, then
    /// `data` will be pinned in memory and unable to be moved.
    pub fn pin(data: T) -> Pin<ParentArc<T>> {
        unsafe { Pin::new_unchecked(ParentArc::new(data)) }
    }

    /// Locks all [`LockWeak`](struct.LockWeak.html) of this instance, it
    /// will prevent all further upgrades until [`unlocked`]. It is advised to call this before
    /// attempting a [`try_into_inner`].
    ///
    /// [`unlocked`]: #method.unlock
    /// [`try_into_inner`]: #method.try_into_inner
    ///
    /// # Examples
    /// ```rust
    /// use parc::ParentArc;
    /// use std::sync::Mutex;
    /// fn main() {
    ///     let parent = ParentArc::new(Mutex::new(0));
    ///     parent.lock(); // LockWeaks are no longer able to upgrade successfully
    ///     assert!(parent.is_locked());
    /// }
    /// ```
    pub fn lock(&self) {
        let lock = &self.inner().lock;
        while lock.compare_and_swap(false, true, Ordering::Release) {}
    }

    /// Check wether the [`LockWeak`](struct.LockWeak.html)s are locked. Since only the Parent can
    /// unlock it is considered a somewhat trustable result.
    pub fn is_locked(&self) -> bool {
        self.inner().lock.load(Ordering::Relaxed)
    }

    /// Unlocks all [`LockWeak`](struct.LockWeak.html) of this [`ParentArc`](struct.ParentArc.html),
    /// this allows for their ugrade to start again.
    ///
    /// # Examples
    /// ```rust
    /// use parc::ParentArc;
    /// use std::sync::Mutex;
    /// fn main() {
    ///     let parent = ParentArc::new(Mutex::new(0));
    ///     
    ///     parent.lock(); // LockWeaks are no longer able to upgrade successfully
    ///     assert!(parent.is_locked());
    ///     
    ///     parent.unlock(); // LockWeaks can upgrade successfully again
    ///     assert!(!parent.is_locked());
    /// }
    /// ```
    pub fn unlock(&self) {
        let lock = &self.inner().lock;
        while lock.compare_and_swap(true, false, Ordering::Release) {}
    }

    /// Downgrade a [`ParentArc`](struct.ParentArc.html) into a [`LockWeak`](struct.LockWeak.html)
    ///
    /// # Examples
    /// ```rust
    /// use parc::{ParentArc, LockWeak};
    /// use std::sync::Mutex;
    /// fn main() {
    ///     let parent = ParentArc::new(Mutex::new(true));
    ///     let weak: LockWeak<_> = ParentArc::downgrade(&parent);
    /// }
    /// ```
    pub fn downgrade(other: &Self) -> LockWeak<T> {
        LockWeak { ptr: other.ptr }
    }

    /// Tries to downgrade a [`ParentArc`](struct.ParentArc.html) into a [`LockWeak`](struct.LockWeak.html) if the inner state allows the latter to upgrade.
    ///
    /// # Examples
    /// ```rust
    /// use parc::{ParentArc, LockWeak};
    /// use std::sync::Mutex;
    /// fn main() {
    ///     let parent = ParentArc::new(Mutex::new(true));
    ///     parent.lock(); // LockWeaks are no longer able to upgrade successfully
    ///     
    ///     if let Some(_) = ParentArc::try_downgrade(&parent) {
    ///         assert!(false);
    ///     }
    /// }
    /// ```
    pub fn try_downgrade(other: &Self) -> Option<LockWeak<T>> {
        if other.inner().lock.load(Ordering::Relaxed) {
            return None;
        }
        Some(LockWeak { ptr: other.ptr })
    }

    /// Blocks the thread until all [`ChildArc`](struct.ChildArc.html) of this instance
    /// have dropped, returning the underlying data.
    ///
    /// # Safety
    ///
    /// This call will indefinitly spin if a child has not droped correctly.
    ///
    /// # Examples
    /// ```rust
    /// use parc::{ParentArc, LockWeak};
    /// use std::sync::Mutex;
    /// fn main() {
    ///     let parent = ParentArc::new(Mutex::new(true));
    ///     
    ///     let weak1: LockWeak<_> = ParentArc::downgrade(&parent);
    ///     let weak2: LockWeak<_> = ParentArc::downgrade(&parent);
    ///     
    ///     let child = weak1.upgrade().unwrap();
    ///     drop(child);
    ///
    ///     let _: Mutex<bool> = parent.block_into_inner();
    /// }
    /// ```
    pub fn block_into_inner(self) -> T {
        let this = self.inner();

        self.lock();
        while this.strong.load(Ordering::Acquire) != 0 {}

        unsafe {
            let elem = ptr::read(&this.data);
            mem::forget(self);
            elem
        }
    }

    /// Non-blocking version of [`block_into_inner`](#method.block_into_inner). It is advised to
    /// call [`lock`](#method.lock) before calling this one, unless you know for sure there are no
    /// [`ChildArc`](struct.ChildArc.html) alive at this instance.
    ///
    /// # Safety
    ///
    /// This will never unwrap `Ok(T)` if a child has not droped correctly.
    ///
    /// # Examples
    /// ```rust
    /// use parc::{ParentArc, LockWeak, TryUnwrapError::*};
    /// use std::sync::Mutex;
    /// fn main() {
    ///     let mut parent = ParentArc::new(Mutex::new(true));
    ///     
    ///     let weak1: LockWeak<_> = ParentArc::downgrade(&parent);
    ///     let weak2: LockWeak<_> = ParentArc::downgrade(&parent);
    ///     
    ///     let child = weak1.upgrade().unwrap();
    ///     
    ///     // Unlocked LockWeaks
    ///     parent = if let Err(WouldLock(parent)) = ParentArc::try_unwrap(parent) {
    ///         parent
    ///     } else {
    ///         unreachable!()
    ///     };
    ///
    ///     // Locked LockWeaks
    ///     parent.lock();
    ///     parent = if let Err(WouldBlock(parent)) = ParentArc::try_unwrap(parent) {
    ///         parent
    ///     } else {
    ///         unreachable!()
    ///     };
    ///     parent.unlock();
    ///
    ///     // Droped children
    ///     drop(child);
    ///     let value: Mutex<bool> = ParentArc::try_unwrap(parent).unwrap();
    /// }
    /// ```
    pub fn try_unwrap(other: Self) -> TryUnwrapResult<T> {
        let this = other.inner();

        if !this.lock.load(Ordering::Relaxed) && this.strong.load(Ordering::Relaxed) > 0 {
            // Check for non-null count and unlock state
            return Err(TryUnwrapError::WouldLock(other));
        }
        if this.strong.load(Ordering::Relaxed) != 0 {
            return Err(TryUnwrapError::WouldBlock(other));
        }

        unsafe {
            let elem = ptr::read(&this.data);
            mem::forget(other);
            Ok(elem)
        }
    }

    fn inner(&self) -> &Womb<T> {
        unsafe { self.ptr.as_ref() } // Ok to do this because we own the data
    }
}

impl<T> AsRef<T> for ParentArc<T> {
    fn as_ref(&self) -> &T {
        &self.inner().data
    }
}

impl<T> ops::Deref for ParentArc<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.inner().data
    }
}

impl<T> Drop for ParentArc<T> {
    fn drop(&mut self) {
        // Wait for all reads to be droped
        let this = self.inner();
        while this.strong.load(Ordering::Acquire) != 0 {}
    }
}

// Inner state shared by all instances: Parent, Weak, Child
struct Womb<T> {
    data: T,
    lock: AtomicBool,
    strong: AtomicUsize,
}

impl<T> Womb<T> {
    fn as_nnptr(data: T) -> NonNull<Self> {
        let x = Box::new(Self {
            data,
            lock: AtomicBool::new(false),
            strong: AtomicUsize::new(0),
        });
        unsafe { NonNull::new_unchecked(Box::into_raw(x)) }
    }
}

/// Weak reference to a [`ParentArc`](struct.ParentArc.html).
///
/// This instance can be locked at any moment, you can try to upgrade it into a
/// [`ChildArc`](struct.ChildArc.html) which assures it can be read until the reader is dropped.
///
/// The typical way to obtain a Weak pointer is to call
/// [`ParentArc::downgrade`](struct.ParentArc.html#method.downgrade).
pub struct LockWeak<T> {
    ptr: NonNull<Womb<T>>,
}

impl<T> LockWeak<T> {
    /// Upgrades this Weak reference into a [`ChildArc`](struct.ChildArc.html) if the data is
    /// unlocked or still owned by the [`ParentArc`](struct.ParentArc.html).
    ///
    /// # Examples
    /// ```rust
    /// use parc::{ParentArc, LockWeak};
    /// use std::sync::Mutex;
    /// fn main() {
    ///     let parent = ParentArc::new(Mutex::new(true));
    ///
    ///     let weak: LockWeak<_> = ParentArc::downgrade(&parent);
    ///     let child = weak.upgrade().unwrap();
    /// }
    /// ```
    pub fn upgrade(&self) -> Option<ChildArc<T>> {
        let this = self.inner()?;

        if this.lock.load(Ordering::Relaxed) {
            return None;
        }

        let mut n = this.strong.load(Ordering::Relaxed);
        loop {
            match this
                .strong
                .compare_exchange_weak(n, n + 1, Ordering::SeqCst, Ordering::Relaxed)
            {
                Ok(_) => break,
                Err(old) => n = old,
            }
        }
        Some(ChildArc::from(self.ptr))
    }

    // Pointer could be voided
    fn inner(&self) -> Option<&Womb<T>> {
        let address = self.ptr.as_ptr() as *mut () as usize;
        if address == core::usize::MAX {
            None
        } else {
            Some(unsafe { self.ptr.as_ref() })
        }
    }
}

unsafe impl<T> Send for LockWeak<T> {}

/// Unclonable owned reference to a [`ParentArc`](struct.ParentArc.html).
///
/// This type can be dereferenced into the underlying data.
///
/// # Examples
/// ```rust
/// use parc::{ParentArc, LockWeak, ChildArc};
/// use std::sync::Mutex;
/// fn main() {
///     let parent = ParentArc::new(Mutex::new(true));
///
///     let weak: LockWeak<_> = ParentArc::downgrade(&parent);
///     let child: ChildArc<_> = weak.upgrade().unwrap();
///
///     assert!(*child.lock().unwrap());
/// }
/// ```
pub struct ChildArc<T> {
    ptr: NonNull<Womb<T>>,
}

impl<T> ChildArc<T> {
    fn from(ptr: NonNull<Womb<T>>) -> Self {
        Self { ptr }
    }
    fn inner(&self) -> &Womb<T> {
        // safe because strong count is up one
        unsafe { self.ptr.as_ref() }
    }
}

impl<T> AsRef<T> for ChildArc<T> {
    fn as_ref(&self) -> &T {
        &self.inner().data
    }
}

impl<T> ops::Deref for ChildArc<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.inner().data
    }
}

impl<T> Drop for ChildArc<T> {
    fn drop(&mut self) {
        let strong = &self.inner().strong;

        let mut n = strong.load(Ordering::Relaxed);
        loop {
            match strong.compare_exchange_weak(n, n - 1, Ordering::SeqCst, Ordering::Relaxed) {
                Ok(_) => break,
                Err(old) => n = old,
            }
        }
    }
}

#[cfg(all(test, not(feature = "no_std")))]
mod tests {
    extern crate std;
    use super::*;
    use std::sync;
    use std::thread;
    use std::vec::Vec;

    #[test]
    fn new() {
        let _ = ParentArc::new(2);
    }

    #[test]
    fn one_simple_thread() {
        let m = ParentArc::new(sync::Mutex::new(0));
        let _ = thread::spawn({
            let weak = ParentArc::downgrade(&m);
            move || match weak.upgrade() {
                Some(mutex) => *mutex.lock().unwrap() += 1,
                None => {}
            }
        })
        .join();
        let _: sync::Mutex<usize> = m.block_into_inner();
    }

    #[test]
    fn join_after_thread() {
        let m = ParentArc::new(sync::Mutex::new(0));
        let h = thread::spawn({
            let weak = ParentArc::downgrade(&m);
            move || match weak.upgrade() {
                Some(mutex) => *mutex.lock().unwrap() += 1,
                None => {}
            }
        });
        let _: sync::Mutex<usize> = m.block_into_inner();
        let _ = h.join();
    }

    #[test]
    fn multiple_threads() {
        let m = ParentArc::new(sync::Mutex::new(0));
        for _ in 0..10 {
            let _ = thread::spawn({
                let weak = ParentArc::downgrade(&m);
                move || match weak.upgrade() {
                    Some(mutex) => *mutex.lock().unwrap() += 1,
                    None => {}
                }
            })
            .join();
        }
        let _: sync::Mutex<usize> = m.block_into_inner();
    }

    #[test]
    fn loop_read_thread() {
        let m = ParentArc::new(sync::Mutex::new(0));
        let h = thread::spawn({
            let weak = ParentArc::downgrade(&m);
            move || loop {
                match weak.upgrade() {
                    Some(mutex) => *mutex.lock().unwrap() += 1,
                    None => break,
                }
            }
        });
        let _: sync::Mutex<usize> = m.block_into_inner();
        let _ = h.join();
    }

    #[test]
    fn many_loop_read_threads() {
        let m = ParentArc::new(sync::Mutex::new(0));

        let mut vh = Vec::new();
        for _ in 0..10 {
            let h = thread::spawn({
                let weak = ParentArc::downgrade(&m);
                move || loop {
                    match weak.upgrade() {
                        Some(mutex) => *mutex.lock().unwrap() += 1,
                        None => break,
                    }
                }
            });
            vh.push(h);
        }

        let _: sync::Mutex<usize> = m.block_into_inner();
        for h in vh {
            let _ = h.join();
        }
    }

    #[test]
    #[should_panic]
    fn one_panic_read_threads() {
        let m = ParentArc::new(sync::atomic::AtomicUsize::new(0));

        let mut vh = Vec::new();
        for i in 0..10 {
            let h = thread::spawn({
                let weak = ParentArc::downgrade(&m);
                move || loop {
                    match weak.upgrade() {
                        Some(at) => {
                            if i != 1 {
                                at.store(1, sync::atomic::Ordering::SeqCst);
                            } else {
                                panic!()
                            }
                        }
                        None => break,
                    }
                }
            });
            vh.push(h);
        }

        //wait for all threads to launch
        thread::sleep(std::time::Duration::new(0, 100));

        let _: sync::atomic::AtomicUsize = m.block_into_inner();

        for h in vh {
            h.join().unwrap(); // panic occurs here
        }
    }
}
