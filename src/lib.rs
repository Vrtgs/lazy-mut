#![no_std]

use cfg_if::cfg_if;

enum State<T, F> {
    Uninit(F),
    Init(T),
    Poisoned,
}

macro_rules! declare_lazy_mut {
    ($($default_mutex: path)?) => {
        pub struct LazyMut<T, F = fn() -> T, M $(= $default_mutex)?> {
            state: lock_api::Mutex<M, State<T, F>>
        }
    };
}

cfg_if! {
    if #[cfg(feature = "std")] {
        extern crate std;
        mod std_lock;
        pub use std_lock::RawStdMutex;
    }
}

cfg_if::cfg_if! {
    if #[cfg(feature = "parking_lot")] {
        declare_lazy_mut!(parking_lot::RawMutex);
    } else if #[cfg(feature = "std")] {
        declare_lazy_mut!(RawStdMutex);
    } else if #[cfg(feature = "spin")] {
        declare_lazy_mut!(spin::Mutex<()>);
    } else {
        declare_lazy_mut!();
    }
}

#[cold]
fn lazy_mut_poisoned() -> ! {
    panic!("LazyMut instance has previously been poisoned")
}

impl<T, F: FnOnce() -> T, M: lock_api::RawMutex> LazyMut<T, F, M> {
    #[inline]
    pub const fn new(f: F) -> Self {
        LazyMut {
            state: lock_api::Mutex::new(State::Uninit(f))
        }
    }

    pub fn into_inner(self) -> Result<T, F> {
        match self.state.into_inner() {
            State::Init(data) => Ok(data),
            State::Uninit(f) => Err(f),
            State::Poisoned => lazy_mut_poisoned(),
        }
    }

    pub fn get_mut(&self) -> lock_api::MappedMutexGuard<'_, M, T> {
        let mut lock = self.state.lock();
        let state = &mut *lock;
        match state {
            State::Init(_) => {}
            State::Uninit(_) => {
                let State::Uninit(f) = core::mem::replace(state, State::Poisoned)
                    // Safety: we just checked and raw that state is Uninit
                    else { unsafe { core::hint::unreachable_unchecked() } };

                let data = f();

                // SAFETY:
                // If the closure accessed this LazyMut somehow
                // it will be caught the panic resulting from the state being poisoned,
                // the mutable borrow for `state` will be invalidated,
                // The state can only be poisoned at this point,
                // so using `write` to skip the destructor
                // of `State` should help the optimizer
                unsafe { core::ptr::write(state, State::Init(data)) }
            }
            State::Poisoned => lazy_mut_poisoned(),
        }

        debug_assert!(matches!(state, State::Init(_)));
        lock_api::MutexGuard::map(lock, |state| {
            let State::Init(ref mut data) = state
                // Safety: if we reached this point then state **must** be init
                else { unsafe { core::hint::unreachable_unchecked() } };
            data
        })
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
                let x = LazyMut::<u64, _, $mutex_ty>::new(|| { 0_u64 });
                std::thread::scope(|s| {
                    for _ in 0..32 {
                        s.spawn(|| for i in 1..=10 {
                            *x.get_mut() += 100;
                            assert!(*x.get_mut() >= 100 * i);
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
