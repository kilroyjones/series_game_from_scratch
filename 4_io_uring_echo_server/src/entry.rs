/// Entry
///
/// This defines iouring entries for the echo server
use crate::bindings::*;
use std::os::unix::io::RawFd;

pub struct Entry<'a> {
    ring: &'a mut io_uring,
}

impl<'a> Entry<'a> {
    /// Create initial Entry
    ///
    /// We create an Entry with a reference to the io_uring instance.
    ///
    pub fn new(ring: &'a mut io_uring) -> Self {
        Entry { ring }
    }

    pub fn set_accept(
        &mut self,
        fd: RawFd,
        addr: *mut sockaddr,
        addrlen: *mut u32,
        user_data: u64,
    ) -> &mut Self {
        let sqe = unsafe { io_uring_get_sqe(self.ring) };
        if !sqe.is_null() {
            unsafe {
                io_uring_prep_accept(sqe, fd, addr, addrlen, 0);
                (*sqe).user_data = user_data;
            }
        }
        self
    }

    pub fn set_recv(
        &mut self,
        fd: RawFd,
        buf: *mut u8,
        len: usize,
        flags: i32,
        user_data: u64,
    ) -> &mut Self {
        let sqe = unsafe { io_uring_get_sqe(self.ring) };
        if !sqe.is_null() {
            unsafe {
                io_uring_prep_recv(sqe, fd, buf as *mut _, len, flags);
                (*sqe).user_data = user_data;
            }
        }
        self
    }

    pub fn set_send(
        &mut self,
        fd: RawFd,
        buf: *const u8,
        len: usize,
        flags: i32,
        user_data: u64,
    ) -> &mut Self {
        let sqe = unsafe { io_uring_get_sqe(self.ring) };
        if !sqe.is_null() {
            unsafe {
                io_uring_prep_send(sqe, fd, buf as *mut _, len, flags);
                (*sqe).user_data = user_data;
            }
        }
        self
    }
}
