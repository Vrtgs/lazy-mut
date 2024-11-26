# lazy-mut

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

