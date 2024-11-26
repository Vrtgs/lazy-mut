use std::boxed::Box;
use std::cell::UnsafeCell;
use std::marker::PhantomData;
use std::mem::MaybeUninit;
use std::pin::Pin;
use std::sync::TryLockError;

#[cold]
fn init_inner_mutex() -> Pin<Box<Inner>> {
    Box::pin(Inner {
        lock: std::sync::Mutex::new(()),
        guard: UnsafeCell::new(MaybeUninit::uninit()),
    })
}

struct Inner {
    lock: std::sync::Mutex<()>,
    guard: UnsafeCell<MaybeUninit<std::sync::MutexGuard<'static, ()>>>,
}

/// A low-level raw mutex implementation for use with the `lock_api` crate.
///
/// `RawStdMutex` is a minimal wrapper around a standard mutex, providing the necessary
/// interface to implement custom locking primitives using the `lock_api` framework.
///
/// # Features
/// - Compatible with the `lock_api` crate for building advanced synchronization primitives.
/// - Ensures safety and synchronization via the internal use of `std::sync::Mutex`.
///
/// # Notes
/// - This struct is intended to be used as a foundational component for custom
///   synchronization abstractions and is not generally used directly in application code.
/// - The implementation follows `lock_api`'s `RawMutex` requirements, such as methods
///   for locking, unlocking, and checking lock status.
///
/// # Safety
/// - Correct usage of this struct requires careful adherence to locking and unlocking
///   sequences to avoid undefined behavior.
/// - Safe usage assumes compliance with the `lock_api` contract.
pub struct RawStdMutex(std::sync::OnceLock<Pin<Box<Inner>>>);

// access to the UnsafeCell is synchronized by the mutex
unsafe impl Send for RawStdMutex {}
unsafe impl Sync for RawStdMutex {}

impl RawStdMutex {
    // Safety:
    // the guard is produced by the mutex `lock` within self
    unsafe fn save_guard(&self, guard: std::sync::MutexGuard<'_, ()>) {
        unsafe {
            #[allow(clippy::needless_lifetimes)]
            unsafe fn extend_life<'a, 'b>(
                x: std::sync::MutexGuard<'a, ()>,
            ) -> std::sync::MutexGuard<'b, ()> {
                unsafe { core::mem::transmute(x) }
            }

            // Safety:
            // user guarantees that the guard was produced by the mutex `lock` within self
            // meaning that the OnceLock has to have been initialized
            let this = &**self.0.get().unwrap_unchecked();

            // Safety:
            // we have exclusive access to our selves
            // and this self reference is valid as it's pinned on the heap
            // therefore it is safe to transmute this lifetime such that it lives longer
            *this.guard.get() = MaybeUninit::new(extend_life(guard))
        }
    }
}

unsafe impl lock_api::RawMutex for RawStdMutex {
    #[allow(clippy::declare_interior_mutable_const)]
    const INIT: Self = Self(std::sync::OnceLock::new());

    type GuardMarker = PhantomData<std::sync::MutexGuard<'static, ()>>;

    fn lock(&self) {
        match self.0.get_or_init(init_inner_mutex).lock.lock() {
            Ok(guard) => unsafe { self.save_guard(guard) },
            Err(_) => unreachable!(),
        }
    }

    fn try_lock(&self) -> bool {
        match self.0.get_or_init(init_inner_mutex).lock.try_lock() {
            Ok(guard) => {
                unsafe { self.save_guard(guard) }
                true
            }
            Err(TryLockError::WouldBlock) => false,
            Err(TryLockError::Poisoned(_)) => unreachable!(),
        }
    }

    unsafe fn unlock(&self) {
        // Safety:
        // caller upholds that we did indeed lock before this
        // therefore there is in fact a mutex guard in this slot
        // and Inner has been initialized
        unsafe { MaybeUninit::assume_init_drop(&mut *self.0.get().unwrap_unchecked().guard.get()) }
    }

    fn is_locked(&self) -> bool {
        let Some(this) = self.0.get() else {
            return false;
        };

        match this.lock.try_lock() {
            Ok(_) => false,
            Err(TryLockError::WouldBlock) => true,
            Err(TryLockError::Poisoned(_)) => unreachable!(),
        }
    }
}
