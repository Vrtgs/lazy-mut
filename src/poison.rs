use cfg_if::cfg_if;

cfg_if! {
    if #[cfg(feature = "std_lock")] {
        cfg_if! {
            if #[cfg(panic = "unwind")] {
                use std::sync::atomic::{AtomicBool, Ordering};
                use std::thread;
            }
        }

        pub(super) type PoisonError<T> = std::sync::PoisonError<T>;
    } else {
        pub(super) struct PoisonError<T> {
            uninhabited: core::convert::Infallible,
            _phantom: core::marker::PhantomData<T>
        }

        impl<T> PoisonError<T> {
            /// Creates a `PoisonError`.
            /// This method may panic if std was built with `panic="abort"`.
            pub fn new(_: T) -> Self {
                unreachable!()
            }

                /// Consumes this error indicating that a lock is poisoned, returning the
                /// underlying guard to allow access regardless.
            pub fn into_inner(self) -> T {
                match self.uninhabited {}
            }
        }

        impl<T> core::fmt::Debug for PoisonError<T> {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                f.debug_struct("PoisonError").finish_non_exhaustive()
            }
        }
    }
}

pub(super) type PoisonLockResult<Guard> = Result<Guard, PoisonError<Guard>>;

pub fn map_result<T, U, F>(result: PoisonLockResult<T>, f: F) -> PoisonLockResult<U>
where
    F: FnOnce(T) -> U,
{
    match result {
        Ok(x) => Ok(f(x)),
        Err(err) => Err(PoisonError::new(f(err.into_inner()))),
    }
}

pub struct Flag {
    #[cfg(all(feature = "std_lock", panic = "unwind"))]
    failed: AtomicBool,
}

impl Flag {
    #[inline(always)]
    pub const fn new() -> Flag {
        Flag {
            #[cfg(all(feature = "std_lock", panic = "unwind"))]
            failed: AtomicBool::new(false),
        }
    }

    #[inline]
    pub fn guard(&self) -> PoisonLockResult<Guard> {
        let ret = Guard {
            #[cfg(all(feature = "std_lock", panic = "unwind"))]
            panicking: thread::panicking(),
        };
        if self.get() {
            Err(PoisonError::new(ret))
        } else {
            Ok(ret)
        }
    }

    #[inline]
    #[cfg(all(feature = "std_lock", panic = "unwind"))]
    pub fn done(&self, guard: Guard) {
        if !guard.panicking && thread::panicking() {
            self.failed.store(true, Ordering::Relaxed);
        }
    }

    #[inline(always)]
    #[cfg(not(all(feature = "std_lock", panic = "unwind")))]
    pub fn done(&self, _guard: Guard) {}

    #[inline]
    #[cfg(all(feature = "std_lock", panic = "unwind"))]
    pub fn get(&self) -> bool {
        self.failed.load(Ordering::Relaxed)
    }

    #[inline(always)]
    #[cfg(not(all(feature = "std_lock", panic = "unwind")))]
    pub fn get(&self) -> bool {
        false
    }

    #[inline]
    pub fn clear(&self) {
        #[cfg(all(feature = "std_lock", panic = "unwind"))]
        self.failed.store(false, Ordering::Relaxed)
    }
}

#[derive(Copy, Clone)]
pub struct Guard {
    #[cfg(all(feature = "std_lock", panic = "unwind"))]
    panicking: bool,
}
