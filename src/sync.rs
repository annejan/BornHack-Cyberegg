//! One shared-cell type for the firmware-vs-simulator mutex split.
//!
//! On firmware (`embassy-core`) a [`SharedCell`] is an
//! `embassy_sync::blocking_mutex::Mutex<M, RefCell<T>>`; on the simulator it
//! is a `std::sync::Mutex<RefCell<T>>` (poison panics via `unwrap`, as before).
//! The raw-mutex kind `M` is chosen per static on firmware ([`Critical`] by
//! default, [`ThreadMode`] for the display state) and ignored on the
//! simulator, so call sites are identical on both targets.

use core::cell::RefCell;

#[cfg(feature = "embassy-core")]
pub use embassy_sync::blocking_mutex::raw::{
    CriticalSectionRawMutex as Critical, ThreadModeRawMutex as ThreadMode,
};

/// Simulator stand-ins for the raw-mutex kinds (never instantiated).
#[cfg(feature = "simulator")]
pub enum Critical {}
#[cfg(feature = "simulator")]
pub enum ThreadMode {}

#[cfg(feature = "embassy-core")]
pub struct SharedCell<T, M = Critical>(
    embassy_sync::blocking_mutex::Mutex<M, RefCell<T>>,
)
where
    M: embassy_sync::blocking_mutex::raw::RawMutex;

#[cfg(feature = "simulator")]
pub struct SharedCell<T, M = Critical>(
    std::sync::Mutex<RefCell<T>>,
    core::marker::PhantomData<M>,
);

#[cfg(feature = "embassy-core")]
impl<T, M: embassy_sync::blocking_mutex::raw::RawMutex> SharedCell<T, M> {
    pub const fn new(v: T) -> Self {
        Self(embassy_sync::blocking_mutex::Mutex::new(RefCell::new(v)))
    }

    /// Run `f` with the lock held, handing it the underlying `RefCell`.
    pub fn lock<R>(&self, f: impl FnOnce(&RefCell<T>) -> R) -> R {
        self.0.lock(f)
    }
}

#[cfg(feature = "simulator")]
impl<T, M> SharedCell<T, M> {
    pub const fn new(v: T) -> Self {
        Self(
            std::sync::Mutex::new(RefCell::new(v)),
            core::marker::PhantomData,
        )
    }

    /// Run `f` with the lock held, handing it the underlying `RefCell`.
    pub fn lock<R>(&self, f: impl FnOnce(&RefCell<T>) -> R) -> R {
        let guard = self.0.lock().unwrap();
        f(&guard)
    }
}

#[cfg(feature = "embassy-core")]
impl<T, M: embassy_sync::blocking_mutex::raw::RawMutex> SharedCell<T, M> {
    /// Lock and borrow the value immutably for the duration of `f`.
    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        self.lock(|c| f(&c.borrow()))
    }

    /// Lock and borrow the value mutably for the duration of `f`.
    pub fn with_mut<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        self.lock(|c| f(&mut c.borrow_mut()))
    }
}

#[cfg(feature = "simulator")]
impl<T, M> SharedCell<T, M> {
    /// Lock and borrow the value immutably for the duration of `f`.
    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        self.lock(|c| f(&c.borrow()))
    }

    /// Lock and borrow the value mutably for the duration of `f`.
    pub fn with_mut<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        self.lock(|c| f(&mut c.borrow_mut()))
    }
}

/// Plain `static` holder for state that is only ever touched from one task,
/// sequentially (the game loop and its draw code). No locking: [`get`] hands
/// out a raw pointer and the caller upholds the single-task access rule. Use
/// [`SharedCell`] instead when more than one task may touch the value.
///
/// [`get`]: SyncCell::get
#[cfg(feature = "game")]
pub(crate) struct SyncCell<T>(core::cell::UnsafeCell<T>);
// SAFETY: access is single-task and sequential by contract (see above).
#[cfg(feature = "game")]
unsafe impl<T> Sync for SyncCell<T> {}
#[cfg(feature = "game")]
impl<T> SyncCell<T> {
    pub(crate) const fn new(v: T) -> Self {
        Self(core::cell::UnsafeCell::new(v))
    }
    pub(crate) fn get(&self) -> *mut T {
        self.0.get()
    }
}

#[cfg(all(test, feature = "simulator"))]
mod tests {
    use super::*;

    static CELL: SharedCell<u32> = SharedCell::new(1);
    static TM: SharedCell<[u8; 2], ThreadMode> = SharedCell::new([0; 2]);

    #[test]
    fn with_and_with_mut() {
        CELL.with_mut(|v| *v += 41);
        assert_eq!(CELL.with(|v| *v), 42);
        TM.with_mut(|a| a[1] = 7);
        assert_eq!(TM.lock(|c| *c.borrow()), [0, 7]);
    }
}
