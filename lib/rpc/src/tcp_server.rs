// Copyright © 2021 University of Colorado. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0 OR MIT

use abomonation::decode;
use alloc::vec::Vec;
use hashbrown::HashMap;
use log::{debug, trace, warn};
use core::cell::RefCell;

use smoltcp::iface::EthernetInterface;
use smoltcp::socket::{SocketHandle, SocketSet, TcpSocket, TcpSocketBuffer};
use smoltcp::time::Instant;

use vmxnet3::smoltcp::DevQueuePhy;

use crate::cluster_api::*;
use crate::rpc::*;
use crate::rpc_api::{RPCHandler, RPCServerAPI};

const RX_BUF_LEN: usize = 8192;
const TX_BUF_LEN: usize = 8192;
const BUF_LEN: usize = 8192;
const HDR_LEN: usize = core::mem::size_of::<RPCHeader>();

pub struct TCPServer<'a> {
    iface: RefCell<EthernetInterface<'a, DevQueuePhy>>,
    sockets: RefCell<SocketSet<'a>>,
    server_handle: SocketHandle,
    handlers: HashMap<RPCType, &'a RPCHandler>,
    hdr_buff: RefCell<Vec<u8>>,
    buff: RefCell<Vec<u8>>,
}

impl TCPServer<'_> {
    pub fn new<'a>(iface: EthernetInterface<'a, DevQueuePhy>, port: u16) -> TCPServer<'_> {
        // Allocate space for server buffers
        let mut buff = Vec::new();
        buff.try_reserve(BUF_LEN).unwrap();
        let mut hdr_buff = Vec::new();
        hdr_buff.try_reserve(HDR_LEN).unwrap();

        // Create SocketSet w/ space for 1 socket
        let mut sock_vec = Vec::new();
        sock_vec.try_reserve(1).unwrap();
        let mut sockets = SocketSet::new(sock_vec);

        // Create RX and TX buffers for the socket
        let mut sock_vec = Vec::new();
        sock_vec.try_reserve(RX_BUF_LEN).unwrap();
        let socket_rx_buffer = TcpSocketBuffer::new(sock_vec);
        let mut sock_vec = Vec::new();
        sock_vec.try_reserve(RX_BUF_LEN).unwrap();
        let socket_tx_buffer = TcpSocketBuffer::new(sock_vec);

        // Initialized the socket and begin listening
        let mut server_sock = TcpSocket::new(socket_rx_buffer, socket_tx_buffer);
        server_sock.listen(port).unwrap();
        debug!("Listening at port {}", port);

        // Add socket to socket set
        let server_handle = sockets.add(server_sock);

        // Initialize the server struct
        let server = TCPServer {
            iface: RefCell::new(iface),
            sockets: RefCell::new(sockets),
            server_handle: server_handle,
            handlers: HashMap::new(),
            hdr_buff: RefCell::new(hdr_buff),
            buff: RefCell::new(buff),
        };
        server
    }

    fn recv(&self, is_hdr: bool, expected_data: usize) -> Result<(), RPCError> {
        let mut total_data_received = 0;

        // Check write size
        if is_hdr {
            assert!(expected_data < self.hdr_buff.borrow().len());
        } else {
            assert!(expected_data < self.buff.borrow().len());
        }

        // Chunked receive into internal buffer
        let mut sockets = self.sockets.borrow_mut();
        loop {
            match self.iface.borrow_mut().poll(&mut sockets, Instant::from_millis(0)) {
                Ok(_) => {}
                Err(e) => {
                    warn!("poll error: {}", e);
                }
            }

            // Check if done
            if total_data_received == expected_data {
                return Ok(());

            // If not done, attempt to receive slice containing remaining data
            } else {
                let mut socket = sockets.get::<TcpSocket>(self.server_handle);
                if socket.can_recv() {
                    let result = if is_hdr {
                        socket.recv_slice(&mut self.hdr_buff.borrow_mut()[total_data_received..expected_data])
                    } else {
                        socket.recv_slice(&mut self.buff.borrow_mut()[total_data_received..expected_data])
                    };

                    if let Ok(bytes_received) = result
                    {
                        total_data_received += bytes_received;
                        trace!(
                            "rcv got {:?}/{:?} bytes",
                            total_data_received,
                            expected_data
                        );
                    } else {
                        warn!("recv_slice failed... trying again?");
                    }
                }
            }
        }
    }

    fn send(&self, is_hdr: bool, expected_data: usize) -> Result<(), RPCError> {
        let mut data_sent = 0;

        // Check send size
        if is_hdr {
            assert!(expected_data <= self.hdr_buff.borrow().len());
        } else {
            assert!(expected_data <= self.buff.borrow().len());
        }
        // Chunked send from internal buffer
        let mut sockets = self.sockets.borrow_mut();
        loop {
            match self.iface.borrow_mut().poll(&mut sockets, Instant::from_millis(0)) {
                Ok(_) => {}
                Err(e) => {
                    warn!("poll error: {}", e);
                }
            }

            if data_sent == expected_data {
                return Ok(());
            } else {
                let mut socket = sockets.get::<TcpSocket>(self.server_handle);
                if socket.can_send() && socket.send_capacity() > 0 && data_sent < expected_data {
                    let end_index = data_sent + core::cmp::min(expected_data - data_sent, socket.send_capacity());
                    debug!("send [{:?}-{:?}]", data_sent, end_index);
                    let result = if is_hdr {
                        socket.send_slice(&self.hdr_buff.borrow()[data_sent..end_index])
                    } else {
                        socket.send_slice(&self.buff.borrow()[data_sent..end_index])
                    };

                    if let Ok(bytes_sent) = result {
                        trace!(
                            "Client sent: [{:?}-{:?}] {:?}/{:?} bytes",
                            data_sent,
                            end_index,
                            end_index,
                            expected_data
                        );
                        data_sent = data_sent + bytes_sent;
                    } else {
                        debug!("send_slice failed... trying again?");
                    }
                }
            }
        }
    }
}

impl ClusterControllerAPI for TCPServer<'_> {
    fn add_client(&mut self) -> Result<NodeId, RPCError> {
        // 'Accept' a client connection
        let mut sockets = self.sockets.borrow_mut();
        loop {
            match self.iface.borrow_mut().poll(&mut sockets, Instant::from_millis(0)) {
                Ok(_) => {}
                Err(e) => {
                    warn!("poll error: {}", e);
                }
            }

            // This is equivalent (more or less) to accept
            let socket = sockets.get::<TcpSocket>(self.server_handle);
            if socket.is_active() && (socket.may_send() || socket.may_recv()) {
                debug!("Connected to client!");
                break;
            }
        }

        // Receive registration information
        self.receive()?;

        // Validate registration header
        let (hdr, _) = unsafe { decode::<RPCHeader>(&mut self.hdr_buff.borrow_mut()) }.unwrap();

        // TODO: modify header, right now just echoes

        // Send response
        self.reply()?;
        
        // Single client server, so all client IDs are 0
        Ok(0)
    }
}

/// RPC server operations
impl<'a> RPCServerAPI<'a> for TCPServer<'a> {
    /// register an RPC func with an ID
    fn register<'c>(&'a mut self, rpc_id: RPCType, handler: &'c RPCHandler) -> Result<(), RPCError>
    where
        'c: 'a,
    {
        if is_reserved(rpc_id) || self.handlers.contains_key(&rpc_id) {
            return Err(RPCError::DuplicateRPCType);
        }
        self.handlers.insert(rpc_id, handler);
        Ok(())
    }

    /// receives next RPC call with RPC ID
    fn receive(&self) -> Result<RPCType, RPCError> {
        // Read header into internal buffer
        self.recv(true, HDR_LEN)?;

        // Parse out RPC Header
        let mut hdr_buff = self.hdr_buff.borrow_mut();
        let (hdr, _) = unsafe { decode::<RPCHeader>(&mut hdr_buff) }.unwrap();

        // Receive the rest of the data
        self.recv(false, hdr.msg_len as usize)?;
        Ok(hdr.msg_type)
    }

    /// replies an RPC call with results
    fn reply(&self) -> Result<(), RPCError> {
        // Send header from internal buffer
        self.send(true, HDR_LEN)?;

        // Parse out RPC Header
        let mut hdr_buff = self.hdr_buff.borrow_mut();
        let (hdr, _) = unsafe { decode::<RPCHeader>(&mut hdr_buff) }.unwrap();

        // Send the rest of the data
        self.send(false, hdr.msg_len as usize)
    }

    /// Run the RPC server
    fn run_server(&mut self) -> Result<(), RPCError> {
        debug!("Starting to run server!");
        self.add_client()?;
        debug!("Added client!");
        loop {
            let rpc_id = self.receive()?;
            match self.handlers.get(&rpc_id) {
                Some(func) => {
                    func(&mut self.hdr_buff.borrow_mut(), &mut self.buff.borrow_mut())?;
                    self.reply()?;
                }
                None => debug!("Invalid RPCType({}), ignoring", rpc_id)
            }
            debug!("Finished handling RPC");
        }
    }
}
