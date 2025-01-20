# lazy_mut

## Overview
The LazyMut library provides a synchronization
primitive for deferred initialization with a single synchronization step.
It is especially useful for scenarios where initialization logic is expensive
or should be deferred until first use. The library supports multiple locking backends,
including std, parking_lot, and spin (depending on feature flags).
This library is #![no_std] compatible, making it suitable for embedded systems and environments
where the standard library is unavailable.


## Features
* thread-safe lazy-initialization structure that wraps an initialization function and synchronizes access to the inner data.
* RAII Guards: Provides scoped locks via LazyMutGuard for safe and automatic unlocking.
* Poisoning Support: Detects panics during initialization and ensures subsequent accesses are safe, marking the instance as poisoned.
* Configurable Locking: Uses RawMutex from different synchronization backends (e.g., `std`, `parking_lot`, or `spin`) depending on enabled features.

```rust
use lazy_mut::LazyMut;

static VICTOR: LazyMut<Vec<u8>> = LazyMut::new(|| vec![1, 2, 3]);

fn main() {
    VICTOR.get_mut().push(10);
    VICTOR.try_get_mut().unwrap().push(10);
    
    assert_eq!(*VICTOR.get_mut(), [1, 2, 3, 10 ,10])
}
```