//!  A user-space thread scheduler with support for synchronization primitives.

#![feature(drain_filter)]
#![feature(linkage)]
#![feature(thread_local)]
#![feature(test)]
#![cfg_attr(not(test), no_std)]

extern crate alloc;

pub mod condvar;
pub mod mutex;
pub mod rwlock;
pub mod scheduler;
pub mod semaphore;
pub mod stack;
pub mod threads;
pub mod tls2;
pub mod upcalls;

/// Type to represent a core id for the scheduler.
type CoreId = usize;

/// Type to represent an IRQ vector.
type IrqVector = u64;
