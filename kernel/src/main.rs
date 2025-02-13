//! The bespin kernel.
//!
//! Here we define the core modules and the main function that the kernel runs after
//! the arch-specific initialization is done (see `arch/x86_64/mod.rs` for an example).

#![no_std]
#![feature(
    intrinsics,
    core_intrinsics,
    llvm_asm,
    lang_items,
    start,
    box_syntax,
    panic_info_message,
    allocator_api,
    global_asm,
    linkage,
    c_variadic,
    box_into_pin,
    maybe_uninit_ref,
    drain_filter,
    alloc_prelude,
    try_reserve,
    new_uninit,
    get_mut_unchecked
)]
#![cfg_attr(
    all(not(test), not(feature = "integration-test"), target_os = "none"),
    deny(warnings)
)]
#![allow(unaligned_references)] // TODO(warnings)
#![allow(unused_attributes)] // TODO(warnings): getting unused attribute #[inline(always)] with rustc > 1.43.0 / abc3073c9 (and it's not clear why)

// TODO(cosmetics): Get rid of these three `extern crate` as we're in edition 2018:
extern crate alloc;
#[macro_use]
extern crate log;
#[macro_use]
extern crate klogger;
extern crate kpi;
#[macro_use]
extern crate static_assertions;

/// The x86-64 platform specific code.
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
#[path = "arch/x86_64/mod.rs"]
pub mod arch;

/// The unix platform specific code.
#[cfg(all(target_arch = "x86_64", target_family = "unix"))]
#[path = "arch/unix/mod.rs"]
pub mod arch;

/// To write unit-tests for our bare-metal code, we include the x86_64
/// arch-specific code on the `unix` platform.
#[cfg(all(test, target_arch = "x86_64", target_family = "unix"))]
#[path = "arch/x86_64/mod.rs"]
pub mod x86_64_arch;

mod error;
mod fs;
mod graphviz;
mod kcb;
mod memory;
mod mlnr;
mod mlnrfs;
mod nr;
#[macro_use]
mod prelude;
mod process;
mod scheduler;
mod stack;

pub mod panic;

/// A kernel exit status.
///
/// This is used to communicate the exit status
/// (if somehow possible) to the outside world.
///
/// If we run in qemu a special ioport can be used
/// to exit the VM and communicate the status to the host.
///
/// # Notes
/// If this type is modified, update the `run.py` script and `tests/integration-test.rs` as well.
#[derive(Copy, Clone, Debug)]
#[repr(u8)]
pub enum ExitReason {
    Ok = 0,
    ReturnFromMain = 1,
    KernelPanic = 2,
    OutOfMemory = 3,
    UnhandledInterrupt = 4,
    GeneralProtectionFault = 5,
    PageFault = 6,
    UserSpaceError = 7,
    ExceptionDuringInitialization = 8,
    UnrecoverableError = 9,
}

/// Kernel entry-point (after initialization has completed).
///
/// # Notes
/// This function is executed from each core (which is
/// different from a traditional main routine).
#[no_mangle]
#[cfg(not(feature = "integration-test"))]
pub fn xmain() {
    let _r = arch::process::spawn("init").expect("Can't launch init");
    crate::scheduler::schedule()
}

// Including a series of other, custom `xmain` routines that get
// selected when compiling for a specific integration test
include!("integration_main.rs");
