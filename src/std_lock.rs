use std::boxed::Box;
use std::cell::UnsafeCell;
use std::mem::MaybeUninit;
use std::pin::Pin;
use std::sync::TryLockError;

#[cold]
fn init_mutex() -> Pin<Box<std::sync::Mutex<()>>> {
    Box::pin(std::sync::Mutex::new(()))
}


pub struct RawStdMutex {
    lock: std::sync::OnceLock<Pin<Box<std::sync::Mutex<()>>>>,
    guard: UnsafeCell<MaybeUninit<std::sync::MutexGuard<'static, ()>>>,
}


// access to the UnsafeCell is synchronized by the mutex
unsafe impl Send for RawStdMutex {}
unsafe impl Sync for RawStdMutex {}

impl RawStdMutex {
    // Safety:
    // the guard is produced by the mutex `lock` within self
    unsafe fn save_guard(&self, guard: std::sync::MutexGuard<'_, ()>) {
        unsafe {
            unsafe fn extend_life<'a, 'b>(x: std::sync::MutexGuard<'a, ()>) -> std::sync::MutexGuard<'b, ()> {
                unsafe { core::mem::transmute(x) }
            }

            // Safety: 
            // we have exclusive access to our selves
            // and this self reference is valid as its pinned on the heap
            // therefore it is safe to transmute this lifetime such that it lives longer
            *self.guard.get() = MaybeUninit::new(extend_life(guard))
        }
    }
}

unsafe impl lock_api::RawMutex for RawStdMutex {
    #[allow(clippy::declare_interior_mutable_const)]
    const INIT: Self = Self {
        lock: std::sync::OnceLock::new(),
        guard: UnsafeCell::new(MaybeUninit::uninit()),
    };

    type GuardMarker = lock_api::GuardNoSend;

    fn lock(&self) {
        match self.lock.get_or_init(init_mutex).lock() {
            Ok(guard) => unsafe { self.save_guard(guard) },
            Err(_) => unreachable!(),
        }
    }

    fn try_lock(&self) -> bool {
        match self.lock.get_or_init(init_mutex).try_lock() {
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
        unsafe { MaybeUninit::assume_init_drop(&mut *self.guard.get()) }
    }

    fn is_locked(&self) -> bool {
        let Some(this) = self.lock.get()
            else { return false };
        
        match this.try_lock() {
            Ok(_) => false,
            Err(TryLockError::WouldBlock) => true,
            Err(TryLockError::Poisoned(_)) => unreachable!(),
        }
    }
}
