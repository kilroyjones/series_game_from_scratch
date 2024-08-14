use std::mem::size_of;
use std::os::unix::io::RawFd;
use std::ptr;

use crate::bindings::*;

#[repr(C)]
// pub struct sockaddr {
//     sa_family: u16,
//     sa_data: [u8; 14],
// }
#[derive(Debug, Clone, Copy)]
pub enum SocketOpcode {
    Accept,
    Connect,
    Recv,
    Send,
    Shutdown,
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Entry {
    pub opcode: SocketOpcode,
    pub fd: RawFd,
    pub addr: *mut sockaddr,
    pub addrlen: *mut u32,
    pub buf: *mut u8,
    pub len: usize,
    pub flags: i32,
    pub user_data: u64,
}

impl Entry {
    pub fn new() -> Self {
        Entry {
            opcode: SocketOpcode::Accept,
            fd: -1,
            addr: ptr::null_mut(),
            addrlen: ptr::null_mut(),
            buf: ptr::null_mut(),
            len: 0,
            flags: 0,
            user_data: 0,
        }
    }

    pub fn set_accept(&mut self, fd: RawFd, addr: *mut sockaddr, addrlen: *mut u32) -> &mut Self {
        self.opcode = SocketOpcode::Accept;
        self.fd = fd;
        self.addr = addr;
        self.addrlen = addrlen;
        self
    }

    pub fn set_connect(&mut self, fd: RawFd, addr: *const sockaddr, addrlen: u32) -> &mut Self {
        self.opcode = SocketOpcode::Connect;
        self.fd = fd;
        self.addr = addr as *mut _;
        self.addrlen = &addrlen as *const _ as *mut _;
        self
    }

    pub fn set_recv(&mut self, fd: RawFd, buf: *mut u8, len: usize, flags: i32) -> &mut Self {
        self.opcode = SocketOpcode::Recv;
        self.fd = fd;
        self.buf = buf;
        self.len = len;
        self.flags = flags;
        self
    }

    pub fn set_send(&mut self, fd: RawFd, buf: *const u8, len: usize, flags: i32) -> &mut Self {
        self.opcode = SocketOpcode::Send;
        self.fd = fd;
        self.buf = buf as *mut _;
        self.len = len;
        self.flags = flags;
        self
    }

    pub fn set_shutdown(&mut self, fd: RawFd, how: i32) -> &mut Self {
        self.opcode = SocketOpcode::Shutdown;
        self.fd = fd;
        self.flags = how;
        self
    }

    pub fn set_user_data(&mut self, user_data: u64) -> &mut Self {
        self.user_data = user_data;
        self
    }
}
