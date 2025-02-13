use alloc::sync::Arc;
use alloc::vec::Vec;
use core::ops::Range;
use core::sync::atomic::{AtomicBool, Ordering};

use apic::ApicDriver;
use bit_field::BitField;
use crossbeam_queue::ArrayQueue;
use lazy_static::lazy_static;
use smallvec::{smallvec, SmallVec};
use x86::apic::{
    ApicId, DeliveryMode, DeliveryStatus, DestinationMode, DestinationShorthand, Icr, Level,
    TriggerMode,
};

use super::memory::BASE_PAGE_SIZE;
use super::process::Ring3Process;
use crate::is_page_aligned;
use crate::memory::vspace::TlbFlushHandle;
use crate::{mlnr, nr};

// In the xAPIC mode, the Destination Format Register (DFR) through the MMIO interface determines the choice of a
// flat logical mode or a clustered logical mode. Flat logical mode is not supported in the x2APIC mode. Hence the
// Destination Format Register (DFR) is eliminated in x2APIC mode.
// The 32-bit logical x2APIC ID field of LDR is partitioned into two sub-fields:
//
// • Cluster ID (LDR[31:16]): is the address of the destination cluster
// • Logical ID (LDR[15:0]): defines a logical ID of the individual local x2APIC within the cluster specified by
//   LDR[31:16].
//
// In x2APIC mode, the 32-bit logical x2APIC ID, which can be read from LDR, is derived from the 32-bit local x2APIC ID:
// Logical x2APIC ID = [(x2APIC ID[19:4] « 16) | (1 « x2APIC ID[3:0])]

lazy_static! {
    static ref IPI_WORKQUEUE: Vec<ArrayQueue<WorkItem>> = {
        let cores = topology::MACHINE_TOPOLOGY.num_threads();
        let mut channels = Vec::with_capacity(cores);
        for _i in 0..cores {
            channels.push(ArrayQueue::new(4));
        }

        channels
    };
}

#[derive(Debug)]
pub enum WorkItem {
    Shootdown(Arc<Shootdown>),
    AdvanceReplica(usize),
}

#[derive(Debug)]
pub struct Shootdown {
    vregion: Range<u64>,
    ack: AtomicBool,
}

impl Shootdown {
    /// Create a new shootdown request.
    pub fn new(vregion: Range<u64>) -> Self {
        debug_assert!(is_page_aligned!(vregion.start));
        debug_assert!(is_page_aligned!(vregion.end));
        Shootdown {
            vregion,
            ack: AtomicBool::new(false),
        }
    }

    /// Acknowledge shootdown to sender/requestor core.
    fn acknowledge(&self) {
        self.ack.store(true, Ordering::Relaxed);
    }

    /// Check if receiver has acknowledged the shootdown.
    pub fn is_acknowledged(&self) -> bool {
        self.ack.load(Ordering::Relaxed)
    }

    /// Flush the TLB entries.
    fn process(&self) {
        // Safe to acknowledge first as we won't return/interrupt
        // before this function completes:
        self.acknowledge();

        let it = self.vregion.clone().step_by(BASE_PAGE_SIZE);
        if it.count() > 20 {
            trace!("flush the entire TLB");
            unsafe { x86::tlb::flush_all() };
        } else {
            let it = self.vregion.clone().step_by(BASE_PAGE_SIZE);
            for va in it {
                trace!("flushing TLB page {:#x}", va);
                unsafe { x86::tlb::flush(va as usize) };
            }
        }
    }
}

pub fn enqueue(gtid: topology::GlobalThreadId, s: WorkItem) {
    trace!("TLB enqueue shootdown msg {:?}", s);
    assert!(IPI_WORKQUEUE[gtid as usize].push(s).is_ok());
}

pub fn dequeue(gtid: topology::GlobalThreadId) {
    match IPI_WORKQUEUE[gtid as usize].pop() {
        Ok(msg) => match msg {
            WorkItem::Shootdown(s) => {
                trace!("TLB channel got msg {:?}", s);
                s.process();
            }
            WorkItem::AdvanceReplica(log_id) => advance_log(log_id),
        },
        Err(_) => { /*IPI request was handled by eager_advance_mlnr_replica()*/ }
    }
}

fn advance_log(log_id: usize) {
    // All metadata operations are done using log 1. So, make sure that the
    // replica has applied all those operation before any other log sync.
    if log_id != 1 {
        match mlnr::MlnrKernelNode::synchronize_log(1) {
            Ok(_) => { /* Simply return */ }
            Err(e) => unreachable!("Error {:?} while advancing the log 1", e),
        }
    }
    match mlnr::MlnrKernelNode::synchronize_log(log_id) {
        Ok(_) => { /* Simply return */ }
        Err(e) => unreachable!("Error {:?} while advancing the log {}", e, log_id),
    }
}

pub fn eager_advance_mlnr_replica() {
    let core_id = topology::MACHINE_TOPOLOGY.current_thread().id;
    match IPI_WORKQUEUE[core_id as usize].pop() {
        Ok(msg) => {
            match &msg {
                WorkItem::Shootdown(_s) => {
                    // If its for TLB shootdown, insert it back into the queue.
                    enqueue(core_id, msg)
                }
                WorkItem::AdvanceReplica(log_id) => advance_log(*log_id),
            }
        }
        Err(_) => {
            let kcb = super::kcb::get_kcb();
            match kcb.arch.mlnr_replica.as_ref() {
                Some(replica) => {
                    let log_id = replica.1.id();
                    // Synchronize NR-replica.
                    let _ignore = nr::KernelNode::<Ring3Process>::synchronize();
                    // Synchronize Mlnr-replica.
                    advance_log(log_id);
                }
                None => unreachable!("eager_advance_mlnr_replica: KCB does not have mlnr_replica!"),
            };
        }
    }
}

pub fn send_ipi_to_apic(apic_id: ApicId) {
    let kcb = super::kcb::get_kcb();
    let mut apic = kcb.arch.apic();

    let icr = Icr::for_x2apic(
        super::irq::MLNR_GC_INIT,
        apic_id,
        DestinationShorthand::NoShorthand,
        DeliveryMode::Fixed,
        DestinationMode::Physical,
        DeliveryStatus::Idle,
        Level::Assert,
        TriggerMode::Edge,
    );

    unsafe { apic.send_ipi(icr) }
}

fn send_ipi_multicast(ldr: u32) {
    let kcb = super::kcb::get_kcb();
    let mut apic = kcb.arch.apic();

    let icr = Icr::for_x2apic(
        super::irq::TLB_WORK_PENDING,
        // TODO(api): this is technically not an APIC id, should probably change the interface
        ApicId::X2Apic(ldr),
        DestinationShorthand::NoShorthand,
        DeliveryMode::Fixed,
        DestinationMode::Logical,
        DeliveryStatus::Idle,
        Level::Assert,
        TriggerMode::Edge,
    );

    unsafe { apic.send_ipi(icr) }
}

/// Runs the TLB shootdown protocol.
///
/// Takes the `TlbFlushHandle` and figures out what cores it needs to send an IPI to.
/// It divides IPIs into clusters to avoid overhead of sending IPIs individually.
/// Finally, waits until all cores have acknowledged the IPI before it returns.
pub fn shootdown(handle: TlbFlushHandle) {
    let my_gtid = {
        let kcb = super::kcb::get_kcb();
        kcb.arch.id()
    };

    // We support up to 16 IPI clusters, this will address `16*16 = 256` cores
    // Cluster ID (LDR[31:16]) is the address of the destination cluster
    // We pre-configure the upper half (cluster ID) of LDR here in the SmallVec
    // by initializing the elements
    let mut cluster_destination: SmallVec<[u32; 16]> = smallvec![
        0 << 16,
        1 << 16,
        2 << 16,
        3 << 16,
        4 << 16,
        5 << 16,
        6 << 16,
        7 << 16,
        8 << 16,
        9 << 16,
        10 << 16,
        11 << 16,
        12 << 16,
        13 << 16,
        14 << 16,
        15 << 16,
    ];

    let mut shootdowns: Vec<Arc<Shootdown>> =
        Vec::with_capacity(topology::MACHINE_TOPOLOGY.num_threads());
    let range = handle.vaddr.as_u64()..(handle.vaddr + handle.frame.size).as_u64();

    for (gtid, include) in handle.core_map.into_iter().enumerate() {
        // TODO: enumerates over all 256 potential entries...
        if include && gtid != my_gtid {
            let apic_id = topology::MACHINE_TOPOLOGY.threads[gtid].apic_id();
            let cluster_addr = apic_id.x2apic_logical_cluster_address();
            let cluster = apic_id.x2apic_logical_cluster_id();

            trace!(
                "Send shootdown to gtid:{} in cluster:{} cluster_addr:{}",
                gtid,
                cluster,
                cluster_addr
            );
            cluster_destination[cluster as usize].set_bit(cluster_addr as usize, true);

            let shootdown = Arc::new(Shootdown::new(range.clone()));
            enqueue(gtid as u64, WorkItem::Shootdown(shootdown.clone()));
            shootdowns.push(shootdown);
        }
    }

    // Notify the cores in all clusters of new work in the queue
    for cluster_ldr in cluster_destination {
        // Do we need to send to anyone inside this cluster?
        if cluster_ldr.get_bits(0..=3) != 0 {
            trace!("send ipi multicast to {}", cluster_ldr);
            send_ipi_multicast(cluster_ldr);
        }
    }

    // Finally, we also need to shootdown our own TLB
    let shootdown = Shootdown::new(range);
    shootdown.process();

    // Wait synchronously on cores to complete
    while !shootdowns.is_empty() {
        shootdowns.drain_filter(|s| s.is_acknowledged());
        core::hint::spin_loop();
    }

    trace!("done with all shootdowns");
}

pub fn advance_replica(gtid: topology::GlobalThreadId, log_id: usize) {
    trace!("Send AdvanceReplica IPI for {} to {}", log_id, gtid);
    let apic_id = topology::MACHINE_TOPOLOGY.threads[gtid as usize].apic_id();

    enqueue(gtid, WorkItem::AdvanceReplica(log_id));
    send_ipi_to_apic(apic_id);
}
