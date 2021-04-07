use alloc::alloc::{AllocError, Layout};
use alloc::boxed::Box;
use alloc::collections::TryReserveError;
use alloc::string::ToString;
use alloc::vec::Vec;
use core::convert::TryInto;
use core::mem;
use core::pin::Pin;

use arrayvec::ArrayVec;
use custom_error_core::custom_error;
use driverkit::net::{PktInfo, RxError, RxdInfo, TxError, TxRx};
use log::{debug, error, info};
use x86::current::paging::{PAddr, VAddr};

use crate::pci::{self, DmaObject};
use crate::reg::*;
use crate::var::*;
use crate::{BoundedU32, BoundedUSize};

#[derive(Eq, PartialEq, Debug, Copy, Clone)]
pub enum QueueId {
    Rx(usize),
    Tx(usize),
}

/// The txq and rxq shared data areas must be allocated contiguously
/// as vmxnet3_driver_shared contains only a single address member
/// for the shared queue data area.
///
/// Must be 128 bytes aligned. Array of vmxnet3_txq_shared is first, followed
/// by an array of vmxnet3_rxq_shared
///
/// This is a mess for rust types so we allocate raw memory and cast it.
struct vmxnet3_trxq_shared {
    nrxq: usize,
    ntxq: usize,
    layout: Layout,
    buffer: *mut u8,
}

impl vmxnet3_trxq_shared {
    fn new(txqsets: usize, rxqsets: usize) -> Result<vmxnet3_trxq_shared, VMXNet3Error> {
        // Safety: Needs to allocate slice that can hold txqsets and rxqsets structs
        // 128 byte alignment requirement by driver
        static_assertions::const_assert_eq!(mem::size_of::<vmxnet3_txq_shared>(), 256);
        static_assertions::const_assert_eq!(mem::size_of::<vmxnet3_rxq_shared>(), 256);

        unsafe {
            let layout = Layout::from_size_align(
                mem::size_of::<vmxnet3_rxq_shared>() * rxqsets
                    + mem::size_of::<vmxnet3_txq_shared>() * txqsets,
                128,
            )
            .expect("Layout for shared queue area was invalid?");

            let ptr = alloc::alloc::alloc_zeroed(layout);
            if ptr.is_null() {
                return Err(VMXNet3Error::OutOfMemory);
            }

            Ok(vmxnet3_trxq_shared {
                nrxq: rxqsets,
                ntxq: txqsets,
                layout: layout,
                buffer: ptr,
            })
        }
    }

    fn rxqs_ref(&self, idx: usize) -> &vmxnet3_rxq_shared {
        if idx >= self.nrxq {
            panic!("Invalid idx parameter.");
        }

        // Safety: Correct allocation, alignment of buffer & alloc_zeroed is initializing this properly
        // indexing as described in comments
        unsafe {
            let txq_end = (self.buffer as *const vmxnet3_txq_shared).add(self.ntxq);
            let rxq_entry = (txq_end as *const vmxnet3_rxq_shared).add(idx);
            &*rxq_entry
        }
    }
    fn rxqs_ref_mut(&mut self, idx: usize) -> &mut vmxnet3_rxq_shared {
        if idx >= self.nrxq {
            panic!("Invalid idx parameter.");
        }

        // Safety: Correct allocation, alignment of buffer & alloc_zeroed is initializing this properly
        // indexing as described in comments
        unsafe {
            let txq_end = (self.buffer as *mut vmxnet3_txq_shared).add(self.ntxq);
            let rxq_entry = (txq_end as *mut vmxnet3_rxq_shared).add(idx);
            &mut *rxq_entry
        }
    }

    fn txqs_ref(&self, idx: usize) -> &vmxnet3_txq_shared {
        if idx >= self.ntxq {
            panic!("Invalid idx parameter.");
        }
        // Safety: Correct allocation, alignment of buffer & alloc_zeroed is initializing this properly
        // indexing as described in comments
        unsafe {
            let txq_entry = (self.buffer as *const vmxnet3_txq_shared).add(idx);
            &*txq_entry
        }
    }

    fn txqs_ref_mut(&mut self, idx: usize) -> &mut vmxnet3_txq_shared {
        if idx >= self.ntxq {
            panic!("Invalid idx parameter.");
        }
        // Safety: Correct allocation, alignment of buffer & alloc_zeroed is initializing this properly
        // indexing as described in comments
        unsafe {
            let txq_entry = (self.buffer as *mut vmxnet3_txq_shared).add(idx);
            &mut *txq_entry
        }
    }
}

impl DmaObject for vmxnet3_trxq_shared {
    fn paddr(&self) -> PAddr {
        PAddr::from(self.buffer as u64 - pci::KERNEL_BASE)
    }

    fn vaddr(&self) -> VAddr {
        VAddr::from(self.buffer as u64)
    }
}

impl Drop for vmxnet3_trxq_shared {
    fn drop(&mut self) {
        unsafe { alloc::alloc::dealloc(self.buffer, self.layout) }
    }
}

#[cfg(test)]
mod tests {
    use super::vmxnet3_trxq_shared;
    use crate::pci::DmaObject;
    use crate::reg::{vmxnet3_rxq_shared, vmxnet3_txq_shared};

    #[test]
    fn test_trxq_shared() {
        let mut x = vmxnet3_trxq_shared::new(2, 3).unwrap();
        for i in 0..2 {
            {
                let r = x.txqs_ref_mut(i);
                assert_eq!(
                    r as *mut vmxnet3_txq_shared as usize,
                    x.vaddr().as_usize() + i * 256
                );
            }
            {
                let r = x.txqs_ref(i);
                assert_eq!(
                    r as *const vmxnet3_txq_shared as usize,
                    x.vaddr().as_usize() + i * 256
                );
            }
        }
        for i in 0..3 {
            {
                let r = x.rxqs_ref_mut(i);
                assert_eq!(
                    r as *mut vmxnet3_rxq_shared as usize,
                    x.vaddr().as_usize() + 2 * 256 + i * 256
                );
            }
            {
                let r = x.rxqs_ref(i);
                assert_eq!(
                    r as *const vmxnet3_rxq_shared as usize,
                    x.vaddr().as_usize() + 2 * 256 + i * 256
                );
            }
        }
    }
}


custom_error! {pub VMXNet3Error
    DeviceNotSupported = "Unknown vmxnet3 device/version",
    InterruptModeNotSupported = "Device requested an interrupt mode that is not supported by driver",
    OutOfMemory  = "Unable to allocate raw memory.",
    OutOfMemory1{ source: TryReserveError }  = "Unable to allocate memory for data-structure",
    OutOfMemory2{ source: AllocError }       = "Unable to allocate object"
}

pub struct VMXNet3 {
    bar0: u64,
    bar1: u64,
    //bar_msix: u64,
    /// Number of transmit queues.
    ntxqsets: BoundedUSize<1, { VMXNET3_MAX_TX_QUEUES }>,
    /// Number of receive queues.
    nrxqsets: BoundedUSize<1, { VMXNET3_MAX_RX_QUEUES }>,

    vmx_flags: u32,

    /// Is link active?
    link_active: bool,
    /// Shared region between driver and host
    ds: Box<DriverShared>,
    /// Queue state that is shared with the device
    qs: vmxnet3_trxq_shared,

    pub rxq: arrayvec::ArrayVec<[RxQueue; VMXNET3_MAX_RX_QUEUES]>,
    pub txq: arrayvec::ArrayVec<[TxQueue; VMXNET3_MAX_TX_QUEUES]>,

    /// Bytes of MAC Address for device
    lladdr: [u8; 6],
}

impl DmaObject for VMXNet3 {}

impl VMXNet3 {
    pub fn new(
        nrx: usize,
        nrxd: usize,
        trx: usize,
        ntxd: usize,
    ) -> Result<Pin<Box<VMXNet3>>, VMXNet3Error> {
        // TODO: supply as arguments/type
        const BUS: u32 = 0x0;
        const DEV: u32 = 0x10;
        const FUN: u32 = 0x0;

        let (bar0, bar1) = unsafe {
            let devline = pci::confread(BUS, DEV, FUN, 0x0);
            assert_eq!(devline, 0x7b015ad, "Sanity check for vmxnet3");

            let bar0 = pci::confread(BUS, DEV, FUN, 0x10);
            let bar1 = pci::confread(BUS, DEV, FUN, 0x14);
            //let bar_msix = pci::confread(BUS, DEV, FUN, 0x7);

            debug!("BAR0 at: {:#x}", bar0);
            debug!("BAR1 at: {:#x}", bar1);
            //debug!("MSI-X at: {:#x}", bar_msi);

            (bar0.into(), bar1.into())
        };

        let ntxqsets = BoundedUSize::<1, VMXNET3_MAX_TX_QUEUES>::new(trx);
        let nrxqsets = BoundedUSize::<1, VMXNET3_MAX_RX_QUEUES>::new(nrx);

        // Allocate queue state that is shared with the device
        let qs = vmxnet3_trxq_shared::new(*ntxqsets, *nrxqsets)?;

        let nintr = (*nrxqsets + *ntxqsets + 1).try_into().unwrap();
        let evintr = *nrxqsets as u8; // The event interrupt is the last vector

        let mut vmx = Pin::new(Box::try_new(VMXNet3 {
            bar0,
            bar1,
            vmx_flags: 0,
            ntxqsets,
            nrxqsets,
            link_active: false,
            ds: Box::new(DriverShared::new(
                nintr,
                evintr,
                qs.paddr().as_u64(),
                qs.layout.size() as u32,
            )),
            qs,
            txq: ArrayVec::new(),
            rxq: ArrayVec::new(),
            lladdr: [0; 6],
        })?);

        // PAddr of vmx struct goes into ds too. Not clear why...
        let pa = vmx.paddr();
        vmx.ds.set_driver_data(pa);

        Ok(vmx)
    }

    /*

        /* Rx queues */
        for (i = 0; i < scctx->isc_nrxqsets; i++) {
            rxq = &sc->vmx_rxq[i];
            rxs = rxq->vxrxq_rs;

            rxs->cmd_ring[0] = rxq->vxrxq_cmd_ring[0].vxrxr_paddr;
            rxs->cmd_ring_len[0] = rxq->vxrxq_cmd_ring[0].vxrxr_ndesc;
            rxs->cmd_ring[1] = rxq->vxrxq_cmd_ring[1].vxrxr_paddr;
            rxs->cmd_ring_len[1] = rxq->vxrxq_cmd_ring[1].vxrxr_ndesc;
            rxs->comp_ring = rxq->vxrxq_comp_ring.vxcr_paddr;
            rxs->comp_ring_len = rxq->vxrxq_comp_ring.vxcr_ndesc;
            rxs->driver_data = vtophys(rxq);
            rxs->driver_data_len = sizeof(struct vmxnet3_rxqueue);
        }
    */

    fn tx_queues_alloc(&mut self) -> Result<(), VMXNet3Error> {
        for i in 0..*self.ntxqsets {
            let txq = TxQueue::new(QueueId::Tx(i), VMXNET3_DEF_TX_NDESC)?;

            // Mirror info in shared queue region:
            let txs = self.qs.txqs_ref_mut(i);
            txs.cmd_ring = txq.vxtxq_cmd_ring.paddr().into();
            txs.cmd_ring_len = txq.vxtxq_cmd_ring.vxtxr_ndesc().try_into().unwrap();
            txs.comp_ring = txq.vxtxq_comp_ring.paddr().into();
            txs.comp_ring_len = txq.vxtxq_comp_ring.vxcr_ndesc().try_into().unwrap();
            txs.driver_data = txq.paddr().into();
            txs.driver_data_len = mem::size_of::<TxQueue>().try_into().unwrap();

            self.txq.push(txq);
        }

        Ok(())
    }

    fn rx_queues_alloc(&mut self) -> Result<(), VMXNet3Error> {
        for i in 0..*self.nrxqsets {
            let rxq = RxQueue::new(QueueId::Rx(i), VMXNET3_DEF_RX_NDESC)?;

            // Mirror info in shared queue region:
            let rxs = self.qs.rxqs_ref_mut(i);
            rxs.cmd_ring[0] = rxq.vxrxq_cmd_ring[0].paddr().into();
            rxs.cmd_ring_len[0] = rxq.vxrxq_cmd_ring[0].vxrxr_ndesc().try_into().unwrap();
            rxs.cmd_ring[1] = rxq.vxrxq_cmd_ring[1].paddr().into();
            rxs.cmd_ring_len[1] = rxq.vxrxq_cmd_ring[1].vxrxr_ndesc().try_into().unwrap();
            rxs.comp_ring = rxq.vxrxq_comp_ring.paddr().into();
            rxs.comp_ring_len = rxq.vxrxq_comp_ring.vxcr_ndesc().try_into().unwrap();
            rxs.driver_data = rxq.paddr().into();
            rxs.driver_data_len = mem::size_of::<RxQueue>().try_into().unwrap();

            self.rxq.push(rxq);
        }

        Ok(())
    }

    pub fn attach_pre(&mut self) -> Result<(), VMXNet3Error> {
        self.tx_queues_alloc()?;
        self.rx_queues_alloc()?;
        let intr_config = self.read_cmd(VMXNET3_CMD_GET_INTRCFG);
        info!("intr_config is set to {:?}", intr_config);
        if intr_config != VMXNET3_IT_AUTO && intr_config != VMXNET3_IT_MSIX {
            return Err(VMXNet3Error::InterruptModeNotSupported);
        }

        self.check_version()?;
        Ok(())
    }

    fn alloc_data(&mut self) {
        // In new(): self.alloc_shared_data()
        // NYI: self.alloc_mcast_table()
        self.init_shared_data();
    }

    fn attach_post(&mut self) {
        if self.rxq.len() > 0 {
            self.vmx_flags |= VMXNET3_FLAG_RSS;
        }

        self.alloc_data();
        //self.set_interrupt_idx();
    }

    fn check_version(&self) -> Result<(), VMXNet3Error> {
        let version = self.read_bar1(VMXNET3_BAR1_VRRS);
        if version & 0x1 == 0 {
            return Err(VMXNet3Error::DeviceNotSupported);
        }
        self.write_bar1(VMXNET3_BAR1_VRRS, 1);

        let version = self.read_bar1(VMXNET3_BAR1_UVRS);
        if version & 0x1 == 0 {
            return Err(VMXNet3Error::DeviceNotSupported);
        }
        self.write_bar1(VMXNET3_BAR1_UVRS, 1);

        Ok(())
    }

    pub fn register(&self) {}

    pub fn msix_intr_assign(&self) {}
    pub fn free_irqs(&self) {}
    pub fn detach(&self) {}
    pub fn shutdown(&self) {}
    pub fn suspend(&self) {}
    pub fn resume(&self) {}

    #[allow(unused)]
    fn read_bar0(&self, offset: u64) -> u32 {
        unsafe { pci::busread(self.bar0, offset) }
    }

    fn write_bar0(&self, offset: u64, data: u32) {
        unsafe { pci::buswrite(self.bar0, offset, data) };
    }

    fn read_bar1(&self, offset: u64) -> u32 {
        unsafe { pci::busread(self.bar1, offset) }
    }

    fn write_bar1(&self, offset: u64, data: u32) {
        unsafe { pci::buswrite(self.bar1, offset, data) };
    }

    fn read_cmd(&self, cmd: u32) -> u32 {
        self.write_cmd(cmd);
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::Release);
        self.read_bar1(VMXNET3_BAR1_CMD)
    }

    fn write_cmd(&self, cmd: u32) {
        self.write_bar1(VMXNET3_BAR1_CMD, cmd);
    }

    pub fn stop(&mut self) {
        self.link_active = false;
        self.write_cmd(VMXNET3_CMD_DISABLE);
        self.write_cmd(VMXNET3_CMD_RESET);
    }

    fn init_shared_data(&mut self) {}

    fn reinit_shared_data(&mut self) {
        self.ds.mtu = *BoundedU32::<1, VMXNET3_MAX_MTU>::new(1500);
        self.ds.ntxqueue = *self.nrxqsets as u8;
        self.ds.nrxqueue = *self.ntxqsets as u8;
        self.ds.upt_features = 0; // TODO: Various

        let (low, high) = self.ds.paddr().split();
        self.write_bar1(VMXNET3_BAR1_DSL, low);
        self.write_bar1(VMXNET3_BAR1_DSH, high);
    }

    fn retrieve_lladdr(&mut self) {
        let low = self.read_cmd(VMXNET3_CMD_GET_MACL);
        let high = self.read_cmd(VMXNET3_CMD_GET_MACH);
        self.lladdr[0] = (low >> 0) as u8;
        self.lladdr[1] = (low >> 8) as u8;
        self.lladdr[2] = (low >> 16) as u8;
        self.lladdr[3] = (low >> 24) as u8;
        self.lladdr[4] = (high >> 0) as u8;
        self.lladdr[5] = (high >> 8) as u8;

        // For testing only:
        // 56:b4:44:e9:62:dc
        debug_assert_eq!(self.lladdr[0], 0x56);
        debug_assert_eq!(self.lladdr[1], 0xb4);
        debug_assert_eq!(self.lladdr[2], 0x44);
        debug_assert_eq!(self.lladdr[3], 0xe9);
        debug_assert_eq!(self.lladdr[4], 0x62);
        debug_assert_eq!(self.lladdr[5], 0xdc);
    }

    fn set_lladdr(&mut self) {
        self.retrieve_lladdr();

        let ml: u32 = (self.lladdr[0] as u32)
            | (self.lladdr[1] as u32) << 8
            | (self.lladdr[2] as u32) << 16
            | (self.lladdr[3] as u32) << 24;
        self.write_bar1(VMXNET3_BAR1_MACL, ml as u32);

        let mh: u32 = (self.lladdr[4] as u32) | (self.lladdr[5] as u32) << 8;
        self.write_bar1(VMXNET3_BAR1_MACH, mh as u32);
    }

    fn reinit_queues(&mut self) {
        for txq in self.txq.iter_mut() {
            unimplemented!();
            //txq.init();
        }

        for rxq in self.rxq.iter_mut() {
            unimplemented!();
            //rxq.init();
        }
    }

    fn enable_device(&mut self) -> bool {
        if self.read_cmd(VMXNET3_CMD_ENABLE) != 0 {
            error!("device enable command failed!");
            return false;
        }

        for idx in 0..self.rxq.len() {
            fn bar0_rxh(q: usize) -> (u64, u64) {
                let q = q as u64;
                (0x800 + q * 8, 0xA00 + q * 8)
            }

            self.write_bar0(bar0_rxh(idx).0, 0);
            self.write_bar0(bar0_rxh(idx).1, 0);
        }

        true
    }

    fn reinit_rxfilters(&mut self) {
        error!("rxfilters currently ignored");
    }

    fn refresh_host_stats(&mut self) {
        self.write_cmd(VMXNET3_CMD_GET_STATS);
    }

    fn link_is_up(&self) -> bool {
        error!(
            "self.read_cmd(VMXNET3_CMD_GET_LINK) = {:#x}",
            self.read_cmd(VMXNET3_CMD_GET_LINK)
        );
        (self.read_cmd(VMXNET3_CMD_GET_LINK) & 0x1) > 0
    }

    fn link_status(&mut self) {
        let link: bool = self.link_is_up();

        if link && !self.link_active {
            info!("Link is active.");
            self.link_active = true;
        } else if !link && self.link_active {
            info!("Link is inactive.");
            self.link_active = false;
        } else {
            error!("Link {} self.link_active {}", link, self.link_active);
        }
    }

    pub fn init(&mut self) {
        self.set_lladdr();
        self.reinit_shared_data();

        // TODO: Not necessary atm
        // self.reinit_queues();

        let r = self.enable_device();
        info!("enabled device {}", r);
        self.reinit_rxfilters();
        self.link_status();
    }
}

impl TxRx for VMXNet3 {
    fn txd_encap(&mut self, pi: PktInfo) -> Result<(), TxError> {
        assert!(
            pi.segments() <= VMXNET3_TX_MAXSEGS,
            "vmxnet3: Packet with too many segments"
        );

        let txq: Option<&TxQueue> = self.txq.get(pi.qsidx);
        txq.map(|txq| {
            //txq.vxtxq_cmd_ring.
        });

        Ok(())
    }

    fn txd_flush(&mut self, qid: u16) {}

    fn txd_credits_update(&mut self, qid: u16, clear: bool) -> Result<(), TxError> {
        Ok(())
    }

    fn isc_rxd_available(&mut self, qsid: u16, cidx: u32) -> Result<(), RxError> {
        Ok(())
    }

    fn rxd_refill(&mut self, qsid: u16, flid: u8, pidx: u32, paddrs: &[u64], vaddrs: &[u64]) {}

    fn rxd_flush(&mut self, qsid: u16, flid: u8, pidx: u32) {}

    fn rxd_pkt_get(&mut self, ri: RxdInfo) -> Result<(), RxError> {
        Ok(())
    }
}
