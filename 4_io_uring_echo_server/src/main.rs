use std::alloc::{alloc, dealloc};
use std::io;
use std::mem::{size_of, zeroed};
use std::net::TcpListener;
use std::os::unix::io::{AsRawFd, RawFd};
use std::ptr::null_mut;

#[allow(non_upper_case_globals)]
#[allow(non_camel_case_types)]
#[allow(non_snake_case)]
#[allow(dead_code)]

mod bindings {
    #[cfg(not(rust_analyzer))]
    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}

mod entry;
mod iouring;

use crate::entry::Entry;
use crate::iouring::IoUring;

use self::bindings::*;

use std::collections::HashMap;
use std::ptr;

use bindings::*;

const QUEUE_DEPTH: u32 = 256;
const BUFFER_SIZE: usize = 1024;

struct UringEchoServer {
    ring: IoUring,
    listener: TcpListener,
    fd_map: HashMap<u64, RawFd>,
}

impl UringEchoServer {
    fn new(port: u16) -> io::Result<Self> {
        let listener = TcpListener::bind(("0.0.0.0", port))?;
        let ring = IoUring::new(QUEUE_DEPTH)?;

        Ok(Self {
            ring,
            listener,
            fd_map: HashMap::new(),
        })
    }

    fn run(&mut self) -> io::Result<()> {
        self.add_accept()?;
        self.ring.submit()?;

        loop {
            match self.ring.wait_completion() {
                Ok(cqe) => self.handle_completion(cqe)?,
                Err(e) => eprintln!("Error waiting for completion: {}", e),
            }
        }
    }

    fn add_accept(&mut self) -> io::Result<()> {
        let mut entry = Entry::new();
        entry
            .set_accept(self.listener.as_raw_fd(), ptr::null_mut(), ptr::null_mut())
            .set_user_data(0);
        self.ring.submit_entry(&entry)
    }

    fn add_read(&mut self, fd: RawFd) -> io::Result<()> {
        let buffer = Box::into_raw(Box::new([0u8; BUFFER_SIZE]));
        let user_data = buffer as u64;

        let mut entry = Entry::new();
        entry
            .set_recv(fd, buffer as *mut u8, BUFFER_SIZE, 0)
            .set_user_data(user_data);

        self.fd_map.insert(user_data, fd);
        self.ring.submit_entry(&entry)
    }

    fn add_write(&mut self, fd: RawFd, buffer: *mut u8, len: usize) -> io::Result<()> {
        let user_data = buffer as u64 | (1 << 63); // Use high bit to mark write operations

        let mut entry = Entry::new();
        entry
            .set_send(fd, buffer as *const u8, len, 0)
            .set_user_data(user_data);

        self.fd_map.insert(user_data & !(1 << 63), fd);
        self.ring.submit_entry(&entry)
    }

    fn handle_completion(&mut self, cqe: io_uring_cqe) -> io::Result<()> {
        let user_data = cqe.user_data;
        let res = cqe.res;

        if user_data == 0 {
            // Accept operation
            if res >= 0 {
                println!("Accepted new connection: {}", res);
                self.add_read(res)?;
            } else {
                eprintln!("Accept failed with error: {}", -res);
            }
            self.add_accept()?;
        } else if user_data & (1 << 63) != 0 {
            // Write operation
            let buffer = (user_data & !(1 << 63)) as *mut u8;
            if res >= 0 {
                println!("Write completed: {} bytes", res);
                if let Some(&fd) = self.fd_map.get(&(user_data & !(1 << 63))) {
                    self.add_read(fd)?; // Queue up another read after successful write
                }
            } else {
                eprintln!("Write failed with error: {}", -res);
            }
            unsafe {
                Box::from_raw(buffer);
            } // Deallocate the buffer
            self.fd_map.remove(&(user_data & !(1 << 63)));
        } else {
            // Read operation
            let buffer = user_data as *mut u8;
            if res > 0 {
                println!("Read {} bytes", res);
                if let Some(&fd) = self.fd_map.get(&user_data) {
                    self.add_write(fd, buffer, res as usize)?;
                } else {
                    eprintln!("Error: FD not found for read operation");
                    unsafe {
                        Box::from_raw(buffer);
                    } // Deallocate the buffer
                }
            } else if res == 0 {
                println!("Connection closed");
                unsafe {
                    Box::from_raw(buffer);
                } // Deallocate the buffer
                self.fd_map.remove(&user_data);
            } else {
                eprintln!("Read failed with error: {}", -res);
                unsafe {
                    Box::from_raw(buffer);
                } // Deallocate the buffer
                self.fd_map.remove(&user_data);
            }
        }

        self.ring.submit()?;
        Ok(())
    }
}

fn main() -> io::Result<()> {
    let mut server = UringEchoServer::new(8080)?;
    println!("Echo server listening on port 8080");
    server.run()
}
