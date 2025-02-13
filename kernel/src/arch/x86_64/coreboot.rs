//! Functionality to boot application cores on x86.
//!
//! This code is closely intertwingled with the assembly code in `start_ap.S`,
//! make sure these two files are and stay in sync.

use alloc::sync::Arc;
use core::sync::atomic::AtomicBool;

use apic::ApicDriver;
use x86::apic::ApicId;
use x86::current::paging::PAddr;

use crate::memory::vspace::MapAction;
use crate::round_up;
use crate::stack::Stack;

use super::kcb;
use super::memory::BASE_PAGE_SIZE;

/// The 16-bit segement where our bootstrap code is.
const X86_64_REAL_MODE_SEGMENT: u16 = 0x0600;

/// The page number in real mode (this is what the IPI wants)
const REAL_MODE_PAGE: u8 = (X86_64_REAL_MODE_SEGMENT >> 8) as u8;

/// The offset, what we have to add to get a physical address.
const REAL_MODE_LINEAR_OFFSET: u16 = X86_64_REAL_MODE_SEGMENT << 4;

/// The corresponding 64-bit address (0 + offset in our case).
const REAL_MODE_BASE: usize = REAL_MODE_LINEAR_OFFSET as usize;

/// Return the address range of `start_ap.S` as (start, end)
///
/// # Note
/// The addresses returned are start and end in kernel space
/// (above KERNEL_BASE, within the relocated ELF file). But
/// when we boot we have to copy the code in a lower address region
/// where a 16-bit mode CPU can execute.
fn ap_code_address_range() -> (PAddr, PAddr) {
    extern "C" {
        /// The first symbol in `start_ap.S`
        static x86_64_start_ap: *const u8;
        /// The very last symbol in `start_ap.S`
        static x86_64_start_ap_end: *const u8;
    }

    unsafe {
        (
            PAddr::from(&x86_64_start_ap as *const _ as u64),
            PAddr::from(&x86_64_start_ap_end as *const _ as u64),
        )
    }
}

/// Calculate the size of the bootstrap code-block in `start_ap.S`
///
/// We do that by taking the difference of the first and last symbol
/// in the file.
fn get_boostrap_code_size() -> usize {
    let (start_address, end_address) = ap_code_address_range();
    let boostrap_code_size = end_address - start_address;
    trace!("boostrap_code_size = {:#x}", boostrap_code_size);

    boostrap_code_size.into()
}

/// Puts the bootstrap code at a well defined segement that an
/// app core (booting in 16-bit mode can read from) (for us this is
/// REAL_MODE_BASE).
///
/// # Safety
/// Let's hope noone else put something there (we should be ok
/// by just excluding everything below 1 MiB from every being
/// allocated).
unsafe fn copy_bootstrap_code() {
    let boot_code_size = get_boostrap_code_size();

    let ap_bootstrap_code: &'static [u8] = get_orignal_bootstrap_code();
    let real_mode_destination: &'static mut [u8] = get_boostrap_code_region();

    let kcb = kcb::get_kcb();
    kcb.arch
        .init_vspace()
        .map_identity(
            PAddr::from(REAL_MODE_BASE as u64),
            round_up!(boot_code_size, BASE_PAGE_SIZE),
            MapAction::ReadWriteExecuteKernel,
        )
        .expect("Can't map bootstrap code");

    real_mode_destination.copy_from_slice(ap_bootstrap_code);
}

/// Initializes the information passed to the APP core by writing
/// overwriting a bunch of declared symbols inside of `start_ap.S`
/// to pass arguments, set the correct stack and page-table
/// and jump to a custom entry function.
///
/// This includes the entry rust function, a pointer
/// to the initial address space, a pointer to the
/// initial stack.
///
/// # Safety
/// To be safe this function should only be invoked
/// during initialization on the BSP core and after we invoked `copy_bootstrap_code`.
///
/// `arg` is read on the new core so we have to ensure whatever they point to
/// lives long enough.
unsafe fn setup_boostrap_code<A>(
    entry_fn: u64,
    arg: Arc<A>,
    initialized: &AtomicBool,
    pml4: u64,
    stack_top: u64,
) {
    // Symbols from `start_ap.S`
    extern "C" {
        /// Bootstrap code jumps to this address after initialization.
        static x86_64_init_ap_absolute_entry: *mut u64;
        /// Bootstrap core switches to this address space during initialization.
        static x86_64_init_ap_init_pml4: *mut u64;
        /// Bootstrap core uses this stack address when starting to execute at `x86_64_init_ap_absolute_entry`.
        static x86_64_init_ap_stack_ptr: *mut u64;

        // TODO: the *const u64 below should be *const A
        // but this crashes rustc:
        // reported at: https://github.com/rust-lang/rust/issues/65025

        /// First argument for entry fn.
        static x86_64_init_ap_arg1: *mut u64;

        /// The ap lock to let us know when the app core currently booting is done
        /// with the initialization code section.
        ///
        /// (And therefore read the content from `x86_64_init_ap_absolute_entry`,
        /// `x86_64_init_ap_init_pml4`, `x86_64_init_ap_stack_ptr` + args and
        /// no longer needs it).
        static x86_64_init_ap_lock: *mut u64;
    }

    // TODO: tried to make the following code less ugly but failed:

    unsafe fn to_bootstrap_pointer(kernel_text_addr: u64) -> *mut u64 {
        let (start_addr, _end_addr) = ap_code_address_range();
        assert!(kernel_text_addr > start_addr.as_u64());
        core::mem::transmute(kernel_text_addr - start_addr.as_u64() + REAL_MODE_BASE as u64)
    }

    // Init function
    let entry_pointer: *mut u64 =
        to_bootstrap_pointer(&x86_64_init_ap_absolute_entry as *const _ as u64);
    *entry_pointer = entry_fn;

    // Arguments
    let arg1_pointer: *mut u64 = to_bootstrap_pointer(&x86_64_init_ap_arg1 as *const _ as u64);
    // We get the address of the `ptr: NonNull<ArcInner<T>>`,
    // the 1st (private) member inside the Arc, and pass it to the app core, there is probably
    // a better/safer and much less ugly way to express this but we just use transmute for now:
    *arg1_pointer = core::mem::transmute::<Arc<A>, u64>(arg);

    // Page-table
    let pml4_pointer: *mut u64 = to_bootstrap_pointer(&x86_64_init_ap_init_pml4 as *const _ as u64);
    *pml4_pointer = pml4;

    // Stack
    let stack_pointer: *mut u64 =
        to_bootstrap_pointer(&x86_64_init_ap_stack_ptr as *const _ as u64);
    *stack_pointer = stack_top;

    // Reset the initialization lock
    // The APP core is supposed to set this to `true` after booting is done...
    let ap_lock_pointer: *mut u64 = to_bootstrap_pointer(&x86_64_init_ap_lock as *const _ as u64);
    *ap_lock_pointer = &*initialized as *const _ as u64;

    trace!(
        "x86_64_init_ap_absolute_entry is at {:p} and set to {:#x}",
        entry_pointer,
        *entry_pointer
    );
    trace!(
        "x86_64_init_ap_init_pml4 is at {:p} and set to {:#x}",
        pml4_pointer,
        *pml4_pointer
    );
    trace!(
        "x86_64_init_ap_stack_ptr is at {:p} and set to {:#x}",
        stack_pointer,
        *stack_pointer
    );
    trace!(
        "x86_64_init_ap_lock is at {:p} and set to {:#x}={:#x}",
        ap_lock_pointer,
        *ap_lock_pointer,
        *((*ap_lock_pointer) as *const u64)
    );
    trace!(
        "x86_64_init_ap_arg1 is at {:p} and set to {:#x}={:#x}",
        arg1_pointer,
        *arg1_pointer,
        *((*arg1_pointer) as *const u64)
    );

    // TODO: probably want a fence here
}

/// Returns a slice to the bootstrap code in the kernel ELF .text section
///
/// Ideally this region of memory shouldn't be modified (it's mapped read-only by
/// default anyways). We first copy it into a low memory region and then do the
/// final adjustments there.
fn get_orignal_bootstrap_code() -> &'static [u8] {
    let (start_address, _end_address) = ap_code_address_range();
    let boot_code_size = get_boostrap_code_size();

    // This is safe since this is in the kernel binary and always only
    // mapped read-only.
    let ap_bootstrap_code: &'static [u8] =
        unsafe { core::slice::from_raw_parts(start_address.as_u64() as *const u8, boot_code_size) };

    ap_bootstrap_code
}

/// Returns a slice to the bootstrap code region from where we boot new cores.
///
/// # Safety
/// Basically this is only safe in the beginning of system initialization
/// and we need to make sure we have memory backing the REAL_MODE_BASE region
/// first.
unsafe fn get_boostrap_code_region() -> &'static mut [u8] {
    let real_mode_destination: &mut [u8] =
        core::slice::from_raw_parts_mut(REAL_MODE_BASE as *mut u8, get_boostrap_code_size());
    real_mode_destination
}

/// Wakes up (resets) a core by sending a sequence of IPIs (INIT, INIT deassert, STARTUP).
///
/// # Notes
/// x86 specification technically requires to sleep between init and startup, but on most
/// modern processors (Xeon Phi being an exception) this is not really necessary.
///
/// # Safety
/// Can easily reset the wrong core (bad for memory safety).
unsafe fn wakeup_core(core_id: ApicId) {
    let kcb = kcb::get_kcb();

    // x86 core boot protocol, without sleeping:
    kcb.arch.apic().ipi_init(core_id);
    kcb.arch.apic().ipi_init_deassert();

    let start = rawtime::Instant::now();
    while start.elapsed().as_millis() > 10 {}

    kcb.arch.apic().ipi_startup(core_id, REAL_MODE_PAGE);
}

/// Starts up the core identified by `core_id`, after initialization it begins
/// to executing in `init_function` and uses `stack` as a stack.
///
/// # Safety
/// You're waking up a core that goes off and does random things
/// (if not being careful), so this can be pretty bad for memory safety.
pub unsafe fn initialize<A>(
    core_id: x86::apic::ApicId,
    init_function: fn(Arc<A>, &AtomicBool),
    args: Arc<A>,
    initialized: &AtomicBool,
    stack: &dyn Stack,
) {
    // Make sure bootsrap code is at correct location in memory
    copy_bootstrap_code();

    // Initialize bootstrap assembly with correct parameters
    let kcb = super::kcb::get_kcb();
    setup_boostrap_code(
        init_function as u64,
        args,
        initialized,
        kcb.arch.init_vspace().pml4_address().into(),
        stack.base() as u64,
    );

    // Send IPIs
    wakeup_core(core_id);
}
