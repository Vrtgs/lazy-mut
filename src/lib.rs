#![no_std]
#![deny(missing_docs)]
#![doc = include_str!("../README.md")]

use crate::poison::PoisonLockResult;
use cfg_if::cfg_if;
use core::ops::{Deref, DerefMut};

enum InitState<T, F> {
    Uninit(F),
    Init(T),
    Poisoned,
}

mod poison;

cfg_if! {
    if #[cfg(feature = "std")] {
        extern crate std;
        mod std_lock;
        pub use std_lock::RawStdMutex;
    }
}

macro_rules! declare_lazy_mut {
    ($default_mutex: path) => {
        /// Alternative to LazyLock<Mutex<T>> with only a single synchronization step
        pub struct LazyMut<T, F = fn() -> T, M = $default_mutex> {
            state: lock_api::Mutex<M, InitState<T, F>>,
            poison_flag: poison::Flag,
        }
    };
}

cfg_if::cfg_if! {
    if #[cfg(feature = "parking_lot")] {
        declare_lazy_mut!(parking_lot::RawMutex);
    } else if #[cfg(feature = "std")] {
        declare_lazy_mut!(RawStdMutex);
    } else if #[cfg(feature = "spin")] {
        declare_lazy_mut!(spin::Mutex<()>);
    } else {
        #[doc(hidden)]
        pub enum NoDefaultMutex {}
        declare_lazy_mut!(NoDefaultMutex);
    }
}

#[cold]
fn lazy_mut_poisoned_init() -> ! {
    panic!("LazyMut instance has been poisoned during initialization")
}

/// An RAII implementation of a "scoped lock" of a LazyMutGuard. When this structure is
/// dropped (falls out of scope), the lock will be unlocked.
///
/// The data protected by the mutex can be accessed through this guard via its
/// `Deref` and `DerefMut` implementations.
#[clippy::has_significant_drop]
#[must_use = "if unused the LazyMut will immediately unlock"]
pub struct LazyMutGuard<'a, T, F, M: lock_api::RawMutex> {
    lazy: &'a LazyMut<T, F, M>,
    poison_guard: poison::Guard,
    marker: core::marker::PhantomData<(&'a mut T, M::GuardMarker)>,
}

impl<T, F, M: lock_api::RawMutex> Deref for LazyMutGuard<'_, T, F, M> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe {
            // Safety:
            // we have exclusive access to the data within LazyMut
            let InitState::Init(ref data) = *self.lazy.state.data_ptr()
            // Safety:
            // we only create LazyMutGuard's that point to init data
            else {
                core::hint::unreachable_unchecked()
            };
            data
        }
    }
}

impl<T, F, M: lock_api::RawMutex> DerefMut for LazyMutGuard<'_, T, F, M> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe {
            // Safety:
            // we have exclusive access to the data within LazyMut
            let InitState::Init(ref mut data) = *self.lazy.state.data_ptr()
            // Safety:
            // we only create LazyMutGuard's that point to init data
            else {
                core::hint::unreachable_unchecked()
            };
            data
        }
    }
}

impl<T, F, M: lock_api::RawMutex> Drop for LazyMutGuard<'_, T, F, M> {
    fn drop(&mut self) {
        self.lazy.poison_flag.done(self.poison_guard)
    }
}

impl<T, F, M: lock_api::RawMutex> core::panic::UnwindSafe for LazyMut<T, F, M> {}

impl<T, F: FnOnce() -> T, M: lock_api::RawMutex> LazyMut<T, F, M> {
    fn force_mut(&self) -> PoisonLockResult<LazyMutGuard<'_, T, F, M>> {
        let mut lock = self.state.lock();
        let state = &mut *lock;
        match state {
            InitState::Init(_) => {}
            InitState::Uninit(_) => unsafe { Self::really_init(state) },
            InitState::Poisoned => lazy_mut_poisoned_init(),
        }

        debug_assert!(matches!(state, InitState::Init(_)));

        poison::map_result(self.poison_flag.guard(), |poison_guard| LazyMutGuard {
            lazy: self,
            marker: core::marker::PhantomData,
            poison_guard,
        })
    }

    /// # Safety
    /// May only be called when the state is `Uninit`.
    #[cold]
    unsafe fn really_init(state: &mut InitState<T, F>) {
        let InitState::Uninit(f) = core::mem::replace(state, InitState::Poisoned)
        // Safety:
        // caller must uphold that this function is only to be called when the state is `Uninit`.
        else {
            unsafe { core::hint::unreachable_unchecked() }
        };

        let data = f();

        // SAFETY:
        // If the closure accessed this LazyMut somehow
        // it will be caught the panic resulting from the state being poisoned,
        // the mutable borrow for `state` will be invalidated,
        // The state can only be poisoned at this point,
        // so using `write` to skip the destructor
        // of `State` should help the optimizer
        unsafe { core::ptr::write(state, InitState::Init(data)) }
    }
}

impl<T, F: FnOnce() -> T, M: lock_api::RawMutex> LazyMut<T, F, M> {
    /// Creates a new `LazyMut` with the provided initialization function.
    #[inline]
    pub const fn new(f: F) -> Self {
        LazyMut {
            state: lock_api::Mutex::new(InitState::Uninit(f)),
            poison_flag: poison::Flag::new(),
        }
    }

    /// Consumes the `LazyMut` and returns the initialized data, or the initialization function if uninitialized.
    pub fn into_inner(self) -> Result<T, F> {
        match self.state.into_inner() {
            InitState::Init(data) => Ok(data),
            InitState::Uninit(f) => Err(f),
            InitState::Poisoned => lazy_mut_poisoned_init(),
        }
    }

    /// Forces initialization if not already initialized and returns a mutable guard to the inner data.
    ///
    /// # Panics
    /// this function panics if another user of this `LazyMut` panicked while holding the `LazyMut`
    /// or when initialization failed (when its poisoned)
    pub fn get_mut(&self) -> LazyMutGuard<'_, T, F, M> {
        self.force_mut().unwrap()
    }

    /// Forces initialization if not already initialized and returns a mutable guard to the inner data.
    ///
    /// # Errors
    /// this function errors if another user of this `LazyMut` panicked while holding the `LazyMut` (when its poisoned)
    /// returns the `LazyMutGuard` wrapped in a Poison Error
    #[cfg(feature = "std")]
    pub fn try_get_mut(&self) -> std::sync::LockResult<LazyMutGuard<'_, T, F, M>> {
        self.force_mut()
    }

    /// Determines whether the `LazyMut` is poisoned.
    /// this checks for
    /// If another thread is active, the `LazyMut` can still become poisoned at any
    /// time. You should not trust a `false` value for program correctness
    /// without additional synchronization.
    pub fn is_poisoned(&self) -> bool {
        matches!(&*self.state.lock(), InitState::Poisoned) || self.poison_flag.get()
    }

    /// Clear the poisoned mutex state from a `LazyMut`.
    ///
    /// If the `LazyMut` is poisoned, it will remain poisoned until this function is called. This
    /// allows recovering from a poisoned state and marking that it has recovered.
    pub fn clear_mutex_poison(&self) {
        self.poison_flag.clear()
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use crate::LazyMut;

    macro_rules! gen_test {
        ($name:ident $mutex_ty:ty) => {
            #[test]
            fn $name() {
                let x = LazyMut::<u64, _, $mutex_ty>::new(|| 0_u64);
                std::thread::scope(|s| {
                    for _ in 0..32 {
                        s.spawn(|| {
                            for i in 1..=10 {
                                let lock = &mut *x.get_mut();
                                *lock += 100;
                                assert!(*lock >= 100 * i);
                            }
                        });
                    }
                });
                assert_eq!(*x.get_mut(), 32 * 10 * 100);
            }
        };
    }

    #[cfg(feature = "std")]
    gen_test!(std_test crate::RawStdMutex);

    #[cfg(feature = "parking_lot")]
    gen_test!(parking_lot_test parking_lot::RawMutex);

    #[cfg(feature = "spin")]
    gen_test!(spin_test spin::Mutex<()>);
}
