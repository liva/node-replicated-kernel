use alloc::boxed::Box;
use alloc::sync::Arc;

use arrayvec::ArrayVec;
use node_replication::Log;
use node_replication::Replica;

use crate::xmain;
use crate::ExitReason;

use crate::kcb::{BootloaderArguments, Kcb};
use crate::memory::{tcache_sp::TCacheSp, Frame, GlobalMemory, GrowBackend, LARGE_PAGE_SIZE};
use crate::nr::{KernelNode, Op};

pub mod debug;
pub mod irq;
pub mod kcb;
pub mod memory;
pub mod process;
pub mod timer;
pub mod vspace;

use process::UnixProcess;

pub use bootloader_shared::*;

pub const MAX_NUMA_NODES: usize = 12;

static mut initialized: bool = false;

pub fn halt() -> ! {
    unsafe { libc::exit(0) };
}

pub fn advance_mlnr_replica() {
    unreachable!("eager_advance_mlnr_replica not implemented for unix");
}

#[start]
pub fn start(_argc: isize, _argv: *const *const u8) -> isize {
    unsafe {
        if initialized {
            return 0;
        } else {
            initialized = true;
        }
    }

    // Note anything lower than Info is currently broken
    // because macros in mem management will do a recursive
    // allocation and this stuff is not reentrant...
    let _r = klogger::init("info");

    lazy_static::initialize(&rawtime::WALL_TIME_ANCHOR);
    lazy_static::initialize(&rawtime::BOOT_TIME_ANCHOR);

    // Allocate 32 MiB and add it to our heap
    let mut tc = TCacheSp::new(0, 0);
    let mut mm = memory::MemoryMapper::new();

    for _i in 0..64 {
        let frame = mm
            .allocate_frame(4096)
            .expect("We don't have vRAM available");
        tc.grow_base_pages(&[frame]).expect("Can't add base-page");
    }

    for _i in 0..5 {
        let frame = mm
            .allocate_frame(2 * 1024 * 1024)
            .expect("We don't have vRAM available");
        tc.grow_large_pages(&[frame]).expect("Can't add large-page");
    }

    let frame = mm
        .allocate_frame(2 * 1024 * 1024 * 1024)
        .expect("We don't have vRAM available");
    let mut annotated_regions = ArrayVec::<[Frame; 64]>::new();
    annotated_regions.push(frame);
    let global_memory = unsafe { Box::new(GlobalMemory::new(annotated_regions).unwrap()) };
    let global_memory_static: &'static GlobalMemory = Box::leak(global_memory);

    // Construct the Kcb so we can access these things later on in the code
    let kernel_args: Box<KernelArgs> = Box::new(Default::default());
    let kernel_binary: &'static [u8] = &[0u8; 1];
    let arch_kcb: kcb::ArchKcb = kcb::ArchKcb::new(Box::leak(kernel_args));
    let cmdline: BootloaderArguments = Default::default();

    let mut kcb = box Kcb::new(&kernel_binary, cmdline, tc, arch_kcb, 0 as topology::NodeId);
    kcb.set_global_memory(global_memory_static);
    debug!("Memory allocation should work at this point...");

    kcb::init_kcb(Box::leak(kcb));
    kcb::get_kcb().init_memfs();

    let log: Arc<Log<Op>> = Arc::new(Log::<Op>::new(LARGE_PAGE_SIZE));
    let bsp_replica = Replica::<KernelNode<UnixProcess>>::new(&log);
    let local_ridx = bsp_replica
        .register()
        .expect("Failed to register with Replica.");
    {
        let kcb = kcb::get_kcb();
        kcb.setup_node_replication(bsp_replica.clone(), local_ridx);
    }

    info!(
        "Started at {} with {:?} since CPU startup",
        *rawtime::WALL_TIME_ANCHOR,
        *rawtime::BOOT_TIME_ANCHOR
    );

    #[cfg(not(test))]
    xmain();

    ExitReason::ReturnFromMain as isize
}
