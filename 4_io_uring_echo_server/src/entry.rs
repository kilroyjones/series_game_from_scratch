/// Entry
///
/// This defines iouring entries for the echo server
///
use crate::bindings::*;
use std::os::unix::io::RawFd;
use std::ptr;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub enum SocketOpcode {
    Accept,
    Recv,
    Send,
    NULL,
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
    /// Create initial Entry
    ///
    /// We create an empty entry with an invalid NULL opcode. This should be
    /// replaced with another value before submitting to the queue.
    ///
    pub fn new() -> Self {
        Entry {
            opcode: SocketOpcode::NULL,
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

    pub fn set_user_data(&mut self, user_data: u64) -> &mut Self {
        self.user_data = user_data;
        self
    }
}
