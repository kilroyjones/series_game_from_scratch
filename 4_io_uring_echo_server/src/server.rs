use crate::entry::Entry;
use crate::iouring::IoUring;
use std::collections::HashMap;
use std::io;
use std::net::TcpListener;
use std::os::unix::io::{AsRawFd, RawFd};
use std::ptr;
use std::time::Duration;

use crate::bindings::*;
const QUEUE_DEPTH: u32 = 256;
const BUFFER_SIZE: usize = 1024;

pub struct EchoServer {
    ring: IoUring,
    listener: TcpListener,
    fd_map: HashMap<u64, RawFd>,
}

impl EchoServer {
    /// Create a new server instance
    ///
    /// This will create a non-blocking TcpListener and the io-uring queue. The
    /// fd_map will be used to track connections.
    ///
    pub fn new(port: u16) -> io::Result<Self> {
        let listener = TcpListener::bind(("0.0.0.0", port))?;
        listener.set_nonblocking(true)?;
        let ring = IoUring::new(QUEUE_DEPTH)?;

        Ok(Self {
            ring,
            listener,
            fd_map: HashMap::new(),
        })
    }

    /// Run the server
    ///
    /// When run, we
    ///  
    pub fn run(&mut self) -> io::Result<()> {
        self.add_accept()?;
        self.ring.submit()?;

        loop {
            match self.ring.peek_completion() {
                Some(cqe) => self.handle_completion(cqe)?,
                None => {
                    // No completion available, submit any pending operations
                    self.ring.submit()?;
                    // Sleep for a short duration to avoid busy-waiting
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
        }
    }

    /// Create a new io-uring entry
    ///
    /// We create an accept empty accept entry and then add the listener's file
    /// descriptor. We set the parameters for addr and addrlen to null since we
    /// don't care about the IP address for now. Later, we'll want to grab these.
    ///
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
            } else if res == EAGAIN as i32 {
                // No new connection available, requeue the accept
                println!("No new connection available");
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
                // println!("Read {} bytes", res);
                let slice = unsafe { std::slice::from_raw_parts(buffer, res as usize) };
                let text = String::from_utf8_lossy(slice);
                println!("Read {} bytes: {}", res, text);
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

        Ok(())
    }
}
