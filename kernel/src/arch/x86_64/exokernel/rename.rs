// Copyright © 2021 University of Colorado. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0 OR MIT

use abomonation::{decode, encode, Abomonation};
use alloc::string::String;
use alloc::vec::Vec;
use core2::io::Result as IOResult;
use core2::io::Write;
use log::{debug, warn};

use rpc::rpc::*;
use rpc::rpc_api::RPCClient;

use crate::arch::exokernel::fio::*;
use crate::cnrfs;

#[derive(Debug)]
pub struct RenameReq {
    pub oldname: String,
    pub newname: String,
}
unsafe_abomonate!(RenameReq: oldname, newname);

pub fn rpc_rename<T: RPCClient>(
    rpc_client: &mut T,
    pid: usize,
    oldname: String,
    newname: String,
) -> Result<(u64, u64), RPCError> {
    debug!("Rename({:?}, {:?})", oldname, newname);
    let req = RenameReq {
        oldname: oldname,
        newname: newname,
    };
    let mut req_data = Vec::new();
    let mut res_data = [0u8; core::mem::size_of::<FIORes>()];
    unsafe { encode(&req, &mut req_data) }.unwrap();
    rpc_client
        .call(
            pid,
            FileIO::FileRename as RPCType,
            &req_data,
            &mut [&mut res_data],
        )
        .unwrap();
    if let Some((res, remaining)) = unsafe { decode::<FIORes>(&mut res_data) } {
        if remaining.len() > 0 {
            return Err(RPCError::ExtraData);
        }
        debug!("Rename() {:?}", res);
        return res.ret;
    } else {
        return Err(RPCError::MalformedResponse);
    }
}

pub fn handle_rename(hdr: &mut RPCHeader, payload: &mut [u8]) -> Result<(), RPCError> {
    // Lookup local pid
    let local_pid = { get_local_pid(hdr.pid) };

    if local_pid.is_none() {
        return construct_error_ret(hdr, payload, RPCError::NoFileDescForPid);
    }
    let local_pid = local_pid.unwrap();

    if let Some((req, _)) = unsafe { decode::<RenameReq>(payload) } {
        debug!(
            "FileRename(oldname={:?}, newname={:?}), local_pid={:?}",
            req.oldname, req.newname, local_pid
        );

        // TODO: fix this
        let mut oldname = req.oldname.clone();
        oldname.push('\0');
        let mut newname = req.newname.clone();
        newname.push('\0');
        let res = FIORes {
            ret: convert_return(cnrfs::MlnrKernelNode::file_rename(
                local_pid,
                oldname.as_ptr() as u64,
                newname.as_ptr() as u64,
            )),
        };
        construct_ret(hdr, payload, res)
    } else {
        warn!("Invalid payload for request: {:?}", hdr);
        construct_error_ret(hdr, payload, RPCError::MalformedRequest)
    }
}
