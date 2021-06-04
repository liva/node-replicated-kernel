// Copyright © 2021 VMware, Inc. All Rights Reserved.
// SPDX-License-Identifier: BSD-2-Clause

/* automatically generated by rust-bindgen 0.58.1 */

#[repr(C)]
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq)]
pub struct PVRDMAGidGlobal {
    pub SubnetPrefix : be64,
    pub InterfaceId : be64
}
#[repr(C)]
#[derive(Copy, Clone)]
pub struct PVRDMAGid {
    pub raw: [u8; 16],
    pub global : PVRDMAGidGlobal
}

/// some tests like this:
// #[test]
// fn bindgen_test_layout_pvrdma_gid__bindgen_ty_1() {
//     assert_eq!(
//         ::core::mem::size_of::<pvrdma_gid__bindgen_ty_1>(),
//         16usize,
//         concat!("Size of: ", stringify!(pvrdma_gid__bindgen_ty_1))
//     );
//     assert_eq!(
//         ::core::mem::align_of::<pvrdma_gid__bindgen_ty_1>(),
//         8usize,
//         concat!("Alignment of ", stringify!(pvrdma_gid__bindgen_ty_1))
//     );
//     assert_eq!(
//         unsafe {
//             &(*(::core::ptr::null::<pvrdma_gid__bindgen_ty_1>())).subnet_prefix as *const _ as usize
//         },
//         0usize,
//         concat!(
//             "Offset of field: ",
//             stringify!(pvrdma_gid__bindgen_ty_1),
//             "::",
//             stringify!(subnet_prefix)
//         )
//     );
//     assert_eq!(
//         unsafe {
//             &(*(::core::ptr::null::<pvrdma_gid__bindgen_ty_1>())).interface_id as *const _ as usize
//         },
//         8usize,
//         concat!(
//             "Offset of field: ",
//             stringify!(pvrdma_gid__bindgen_ty_1),
//             "::",
//             stringify!(interface_id)
//         )
//     );
// }
// #[test]
// fn bindgen_test_layout_pvrdma_gid() {
//     assert_eq!(
//         ::core::mem::size_of::<pvrdma_gid>(),
//         16usize,
//         concat!("Size of: ", stringify!(pvrdma_gid))
//     );
//     assert_eq!(
//         ::core::mem::align_of::<pvrdma_gid>(),
//         8usize,
//         concat!("Alignment of ", stringify!(pvrdma_gid))
//     );
//     assert_eq!(
//         unsafe { &(*(::core::ptr::null::<pvrdma_gid>())).raw as *const _ as usize },
//         0usize,
//         concat!(
//             "Offset of field: ",
//             stringify!(pvrdma_gid),
//             "::",
//             stringify!(raw)
//         )
//     );
//     assert_eq!(
//         unsafe { &(*(::core::ptr::null::<pvrdma_gid>())).global as *const _ as usize },
//         0usize,
//         concat!(
//             "Offset of field: ",
//             stringify!(pvrdma_gid),
//             "::",
//             stringify!(global)
//         )
//     );
}

impl Default for PVRDMAGid {
    fn default() -> Self {
        unsafe { ::core::mem::zeroed() }
    }
}


/// defines the userd link layer for the PVRDMA device
#[repr(C)]
pub enum PVRDMALinkLayer {
    Uspecified,
    Infiniband,
    Ethernet
}

#[repr(C)]
pub enum PVRDMAMtu {
    Mtu256 = 1,
    Mtu512 = 2,
    Mtu1024 = 3,
    Mtu2048 = 4,
    Mtu4096 = 5,
}

pub fn PVRDMAMTUtoInteger(mtu : PVRDMAMtu) -> u32 {
    match mtu {
        PVRDMAMtu::Mtu256 => 256,
        PVRDMAMtu::Mtu512 => 512,
        PVRDMAMtu::Mtu1024 => 1024,
        PVRDMAMtu::Mtu2048 => 2048
        PVRDMAMtu::Mtu4096 => 4096
    }
}

pub fn PVRDMAIntegerToMTU(mtu : u32) -> PVRDMAMtu {
    match mtu {
        256  => PVRDMAMtu::Mtu256,
        512  => PVRDMAMtu::Mtu512,
        1024 => PVRDMAMtu::Mtu1024,
        2048 => PVRDMAMtu::Mtu2048,
        4096 => PVRDMAMtu::Mtu4096,
        _    => PVRDMAMtu::Mtu4096
    }
}

#[repr(C)]
pub enum PVRDMAPortState {
    Nop          = 0,
    Down         = 1,
    Init         = 2,
    Armed        = 3,
    Active       = 4,
    ActiveDefer = 5
}

#[repr(C)]
pub enum PVRDMAPortCapFlagse {
    Sm                      = 1 <<  1,
    NoticeSup               = 1 <<  2,
    TrapSup                 = 1 <<  3,
    OptIpdSupP              = 1 <<  4,
    AutoMigrSup             = 1 <<  5,
    SlMapSupP               = 1 <<  6,
    MkeyNvram               = 1 <<  7,
    PkeyNvram               = 1 <<  8,
    LedInfoSup              = 1 <<  9,
    SmDisabled              = 1 << 10,
    SysImageGuidSup         = 1 << 11,
    PkeySwExtPortTrapSup    = 1 << 12,
    ExtendedSpeedsSup       = 1 << 14,
    CmSup                   = 1 << 16,
    SnmpTunnelSup           = 1 << 17,
    ReinitSup               = 1 << 18,
    DeviceMgmtSup           = 1 << 19,
    VendorClassSup          = 1 << 20,
    DrNoticeSup             = 1 << 21,
    CapMaskNoticeSup        = 1 << 22,
    BootMgmtSup             = 1 << 23,
    LinkLatencySup          = 1 << 24,
    ClientRegSup            = 1 << 25,
    IpBasedGids             = 1 << 26,
    CapMaxFlags             = 1 << 26,
};

#[repr(C)]
pub enum PVRDMAPortWidth {
    Width1x  = 1,
    Width4x  = 2,
    Width8x  = 4,
    Width12x = 8,
}

pub pub PVRDMAWidthToInteger(w : PVRDMAPortWidth) -> u32 {
    match w {
        Width1x  => 1,
        Width4x  => 2,
        Width8x  => 4,
        Width12x => 8,
    }
}

#[repr(C)]
pub enum PVRDMAPortSpeed {
    Sdr   = 1,
    Ddr   = 2,
    Qdr   = 4,
    Fdr10 = 8,
    Fdr   = 16,
    Edr   = 32
}

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct PVRDMAPortAttr {
    pub state: PVRDMAPortState,
    pub max_mtu: PVRDMAMtu,
    pub active_mtu: PVRDMAMtu,
    pub gid_tbl_len: u32,
    pub port_cap_flags: u32,
    pub max_msg_sz: u32,
    pub bad_pkey_cntr: u32,
    pub qkey_viol_cntr: u32,
    pub pkey_tbl_len: u16,
    pub lid: u16,
    pub sm_lid: u16,
    pub lmc: u8,
    pub max_vl_num: u8,
    pub sm_sl: u8,
    pub subnet_timeout: u8,
    pub init_type_reply: u8,
    pub active_width: u8,
    pub active_speed: u8,
    pub phys_state: u8,
    pub reserved: [u8; 2],
}

impl Default for PVRDMAPortAttr {
    fn default() -> Self {
        unsafe { ::core::mem::zeroed() }
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct PVRDmaGlobalRoute {
    pub dgid: PVRDMAGid,
    pub flow_label: u32,
    pub sgid_index: u8,
    pub hop_limit: u8,
    pub traffic_class: u8,
    pub reserved: u8,
}

impl Default for PVRDmaGlobalRoute {
    fn default() -> Self {
        unsafe { ::core::mem::zeroed() }
    }
}


#[repr(C)]
#[derive(Copy, Clone)]
pub struct PVRDMAGrh {
    pub version_tclass_flow: be32,
    pub paylen: be16,
    pub next_hdr: u8,
    pub hop_limit: u8,
    pub sgid: PVRDMAGid,
    pub dgid: PVRDMAGid,
}

impl Default for PVRDMAGrh {
    fn default() -> Self {
        unsafe { ::core::mem::zeroed() }
    }
}

#[repr(C)]
pub enum PVRDMAAhFlags {
    AhGrh = 1
}


#[repr(C)]
pub enum PVRDMARate {
    PortCurrent = 0;
    Rate25Gbps = 2;
    Rate5Gbps = 5;
    Rate10Gbps = 3;
    Rate20Gbps = 6;
    Rate30Gbps = 4;
    Rate40Gbps = 7;
    Rate60Gbps = 8;
    Rate80Gbps = 9;
    Rate120Gbps = 10;
    Rate14Gbps = 11;
    Rate56Gbps = 12;
    Rate112Gbps = 13;
    Rate168Gbps = 14;
    Rate25Gbps = 15;
    Rate100Gbps = 16;
    Rate200Gbps = 17;
    Rate300Gbps = 18;
}


#[repr(C)]
#[derive(Copy, Clone)]
pub struct PVRDMAAhAttr {
    pub grh: PVRDmaGlobalRoute,
    pub dlid: u16,
    pub vlan_id: u16,
    pub sl: u8,
    pub src_path_bits: u8,
    pub static_rate: u8,
    pub ah_flags: u8,
    pub port_num: u8,
    pub dmac: [u8; 6],
    pub reserved: u8,
}

impl Default for PVRDMAAhAttr {
    fn default() -> Self {
        unsafe { ::core::mem::zeroed() }
    }
}


#[repr(C)]
pub enum PVRDMACqNotifyFlags {
    Solicited = 1,
    NextComp = 2,
    SolicitedMask = 3,
    ReportMissedEvents = 4
}


#[repr(C)]
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq)]
pub struct PVRDMAQpCap {
    pub max_send_wr: u32,
    pub max_recv_wr: u32,
    pub max_send_sge: u32,
    pub max_recv_sge: u32,
    pub max_inline_data: u32,
    pub reserved: u32,
}

#[repr(C)]
pub enum PVRDMASigType {
    AllWr,
    ReqWr
}

#[repr(C)]
pub enum PVRDMAQpType {
    Smi = 0,
    Gsi = 1,
    Rc = 2,
    Uc = 3,
    Ud = 4,
    RawIPv6 = 5,
    RawEtherType = 6,
    RawPacket = 8,
    XrcIni = 9,
    XrcTgt = 10,
    Max = 11
}


#[repr(C)]
pub enum PVRDMAQpCreateFlags {
    CreateIPoPVRDMA = 1,
    CreateMulticastLoopback = 2
}


#[repr(C)]
pub enum PVRDMAQpAttrMask {
    State               = 1 << 0,
    CurState            = 1 << 1,
    EnSqdAsyncNotify    = 1 << 2,
    AccessFlags         = 1 << 3,
    PkeyIndex           = 1 << 4,
    Port                = 1 << 5,
    QKey                = 1 << 6,
    Av                  = 1 << 7,
    PathMtu             = 1 << 8,
    Timeout             = 1 << 9,
    RetryCnt            = 1 << 10,
    RnrRetry            = 1 << 11,
    RqPsn               = 1 << 12,
    MaxQpRdAtomic       = 1 << 13,
    AltPath             = 1 << 14,
    MinRnrTimer         = 1 << 15,
    SqPsn               = 1 << 16,
    MaxDestRdAtomic     = 1 << 17,
    PathMigState        = 1 << 18,
    Cap                 = 1 << 19,
    DestQpn             = 1 << 20,
    AttrMaskMax         = 1 << 20,
}

#[repr(C)]
pub enum PVRDMAQpState {
    Reset,
    Init,
    Rtr,
    Rts,
    Sqd,
    Sqe,
    Err
}

#[repr(C)]
pub enum PVRDMAMigState {
    Migrated,
    Rearm,
    Armed
}

#[repr(C)]
pub enum PVRDMAMwType {
    Type1 = 1,
    Type2 = 2
}


#[repr(C)]
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq)]
pub struct PVRDMASrqAttr {
    pub max_wr: u32,
    pub max_sge: u32,
    pub srq_limit: u32,
    pub reserved: u32,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct PVRDMAQpAttr {
    pub qp_state: PVRDMAQpState,
    pub cur_qp_state: PVRDMAQpState,
    pub path_mtu: PVRDMAMtu,
    pub path_mig_state: PVRDMAMigState,
    pub qkey: u32,
    pub rq_psn: u32,
    pub sq_psn: u32,
    pub dest_qp_num: u32,
    pub qp_access_flags: u32,
    pub pkey_index: u16,
    pub alt_pkey_index: u16,
    pub en_sqd_async_notify: u8,
    pub sq_draining: u8,
    pub max_rd_atomic: u8,
    pub max_dest_rd_atomic: u8,
    pub min_rnr_timer: u8,
    pub port_num: u8,
    pub timeout: u8,
    pub retry_cnt: u8,
    pub rnr_retry: u8,
    pub alt_port_num: u8,
    pub alt_timeout: u8,
    pub reserved: [u8; 5],
    pub cap: PVRDMAQpCap,
    pub ah_attr: PVRDMAAhAttr,
    pub alt_ah_attr: PVRDMAAhAttr,
}
impl Default for pvrdma_qp_attr {
    fn default() -> Self {
        unsafe { ::core::mem::zeroed() }
    }
}

#[repr(C)]
pub enum PVRDMASendFlags {
    Fence = 1,
    Signaled = 2,
    Solicited = 4,
    Inline = 8,
    IpCSum = 16,
    FlagsMax = 16
}

#[repr(C)]
pub enum PVRDMAAccessFlags {
    LocalWrite = 1,
    RemoteWrite = 2,
    RemoteREad = 4,
    RemoteAtomic = 8,
    MwBind = 16,
    ZeroBased = 32,
    OnDemand = 64,
    FlagsMax 64
}