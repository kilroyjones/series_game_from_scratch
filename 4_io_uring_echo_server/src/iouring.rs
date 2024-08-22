/// IoUring
///
/// This crate sits between our IoUring instance and the bindings from liburing.
/// It uses a limited subset of iouring's functionality. Just enough to get a basic
/// echo server running.
///
use crate::bindings::*;
use crate::entry::Entry;
use crate::entry::SocketOpcode;
use std::io;
use std::mem::zeroed;
use std::ptr;

pub struct IoUring {
    ring: io_uring,
}

impl IoUring {
    /// Creates an io-uring instance
    ///
    /// We create a default (zeroed) out queue. The size of this queue is
    /// dependent on the version of the kernel you're using.
    ///
    pub fn new(entries: u32) -> io::Result<Self> {
        let mut ring: io_uring = unsafe { zeroed() };
        let ret = unsafe { io_uring_queue_init(entries, &mut ring, 0) }; // This will return and -errno upon failure

        if ret < 0 {
            return Err(io::Error::from_raw_os_error(-ret));
        }
        Ok(Self { ring })
    }

    /// Add entry to the queue
    ///
    /// What happens here is we first get a pointer to part of the shared memory
    /// between user and kernel space, then use one of the prep functions to
    /// prepare the space with our entry data.
    ///
    /// The last part, user_data, is a way to tag such that we know when a
    /// specific one has completed. We're not really making use of this.
    ///
    pub fn create_entry(&mut self, entry: &Entry) -> io::Result<()> {
        // Get a pointer to an empty submission queue entry in the shared memoery.
        let sqe = unsafe { io_uring_get_sqe(&mut self.ring) };

        // If null, then the queue is full
        if sqe.is_null() {
            return Err(io::Error::new(io::ErrorKind::Other, "Failed to get SQE"));
        }

        // Fill the submission queue entry
        unsafe {
            match entry.opcode {
                SocketOpcode::Accept => {
                    io_uring_prep_accept(sqe, entry.fd, entry.addr, entry.addrlen, 0);
                }
                SocketOpcode::Recv => {
                    io_uring_prep_recv(
                        sqe,
                        entry.fd,
                        entry.buf as *mut _,
                        entry.len as usize,
                        entry.flags,
                    );
                }
                SocketOpcode::Send => {
                    io_uring_prep_send(
                        sqe,
                        entry.fd,
                        entry.buf as *const _,
                        entry.len,
                        entry.flags,
                    );
                }
                SocketOpcode::NULL => {}
            }

            // Add the user_data
            (*sqe).user_data = entry.user_data;
        }

        Ok(())
    }

    /// Submits the entries
    ///
    /// We can create multiple or a single entry before submitting.
    ///
    pub fn submit(&mut self) -> io::Result<usize> {
        let ret = unsafe { io_uring_submit(&mut self.ring) };

        if ret < 0 {
            Err(io::Error::from_raw_os_error(-ret))
        } else {
            Ok(ret as usize)
        }
    }

    /// Peeks the completion queue for completions
    ///
    /// This creates space for a completion queue entry (CQE), then attempt to
    /// fill it with a pointer to a completed entry. It either returns None or
    /// will read the entry based on the returned pointer to return and then
    /// register it as "seen" so that it can be cleaned up.
    ///
    pub fn peek_completion(&mut self) -> Option<io_uring_cqe> {
        let mut cqe: *mut io_uring_cqe = ptr::null_mut();
        let ret = unsafe { io_uring_peek_cqe(&mut self.ring, &mut cqe) };

        if ret < 0 || cqe.is_null() {
            None
        } else {
            let result = unsafe { ptr::read(cqe) };
            unsafe { io_uring_cqe_seen(&mut self.ring, cqe) };
            Some(result)
        }
    }
}

impl Drop for IoUring {
    fn drop(&mut self) {
        unsafe { io_uring_queue_exit(&mut self.ring) };
    }
}
