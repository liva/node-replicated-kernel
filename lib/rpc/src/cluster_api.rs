// Copyright © 2021 University of Colorado. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0 OR MIT

use core::result::Result;

use crate::rpc::{RPCType, RPCError};

/// Node ID for servers/clients
pub type NodeId = u64;

pub const CLUSTER_OPERATION: RPCType = 0;

pub fn is_reserved(rpc_id: RPCType) -> bool {
    return rpc_id == CLUSTER_OPERATION;
}

pub trait ClusterControllerAPI {
    ///  Controller-side implementation for LITE join_cluster()
    fn add_client(&mut self) -> Result<NodeId, RPCError>;
}

pub trait ClusterClientAPI {
    /// Register with controller, analogous to LITE join_cluster()
    fn join_cluster(&mut self) -> Result<NodeId, RPCError>;
}
