use crate::entry::Entry;
use crate::iouring::IoUring;
use std::collections::HashMap;
use std::env::join_paths;
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
    /// When run, we first add the listener to the shared memory space, then we
    /// submit it to the queue, after which we start looping.  The queue is
    /// peeked for completions which are then handled.
    ///
    /// The sleep is to keep us from hammering too hard.
    ///  
    pub fn run(&mut self) -> io::Result<()> {
        self.add_accept()?; //
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
        self.ring.create_entry(&entry)
    }

    /// Receive information
    ///
    /// We create a buffer to store the incoming information and a recv entry
    /// and then use the unique memory address of that buffer as the key in our
    /// hash with the value as the fd.
    ///
    fn add_recv(&mut self, fd: RawFd) -> io::Result<()> {
        let buffer = Box::into_raw(Box::new([0u8; BUFFER_SIZE]));
        let user_data = buffer as u64; // Get unique ID for associated file descriptor

        let mut entry = Entry::new();
        entry
            .set_recv(fd, buffer as *mut u8, BUFFER_SIZE, 0)
            .set_user_data(user_data); // Store ID in entry

        self.fd_map.insert(user_data, fd);
        self.ring.create_entry(&entry)
    }

    /// Send information
    ///
    /// Here we use the most significant bit set to 1 to indicate a send
    /// operation. This is also stored in the user data. The top 16 bits of a
    /// u64 are generally reserved, so this shouldn't conflict with the receive
    /// operation above and shouldn't cause issue when doing the & (and)
    /// operation.
    ///
    fn add_send(&mut self, fd: RawFd, buffer: *mut u8, len: usize) -> io::Result<()> {
        // Take the given point and make 1 the most significant bit.
        let user_data = buffer as u64 | (1 << 63);

        // Create the entry, using the 1 as MSB to indicate a send
        let mut entry = Entry::new();
        entry
            .set_send(fd, buffer as *const u8, len, 0)
            .set_user_data(user_data);

        // Remove the 1 as MSB and use the data as the key -> file descriptor
        self.fd_map.insert(user_data & !(1 << 63), fd);
        self.ring.create_entry(&entry)
    }

    /// Handles completed queue entries
    ///
    /// Takes in an entry from the completion queue and processes:
    ///     1. An accept
    ///     2. Write
    ///     3. Read
    ///     4. Closed connection (from user side)
    ///     5. Error
    ///
    fn handle_completion(&mut self, cqe: io_uring_cqe) -> io::Result<()> {
        let user_data = cqe.user_data;
        let res = cqe.res;

        if user_data == 0 {
            self.handle_accept(res)?;
        } else if user_data & (1 << 63) != 0 {
            self.handle_read(res, user_data)?;
        } else {
            self.handle_write(res, user_data)?;
        }

        Ok(())
    }

    fn handle_accept(&mut self, res: i32) -> io::Result<()> {
        if res >= 0 {
            println!("Accepted new connection: {}", res);
            self.add_recv(res)?;
        } else if res == EAGAIN as i32 {
            println!("No new connection available");
        } else {
            eprintln!("Accept failed with error: {}", -res);
        }

        // We requeue to keep listening for new connections
        self.add_accept()
    }

    fn handle_read(&mut self, res: i32, user_data: u64) -> io::Result<()> {
        // Get the pointer back
        let buffer = (user_data & !(1 << 63)) as *mut u8;
        if res >= 0 {
            println!("Send completed: {} bytes", res);
            // Checkes to see if there's still a connection, if so we queue
            // up to receive another message.
            if let Some(&fd) = self.fd_map.get(&(user_data & !(1 << 63))) {
                self.add_recv(fd)?;
            }
        } else {
            eprintln!("Write failed with error: {}", -res);
        }

        // Deallocate the buffer
        unsafe {
            let _ = Box::from_raw(buffer);
        }
        self.fd_map.remove(&(user_data & !(1 << 63)));
        Ok(())
    }

    fn handle_write(&mut self, res: i32, user_data: u64) -> io::Result<()> {
        let buffer = user_data as *mut u8;
        if res > 0 {
            let slice = unsafe { std::slice::from_raw_parts(buffer, res as usize) };
            let text = String::from_utf8_lossy(slice);
            println!("Read {} bytes: {}", res, text);

            if let Some(&fd) = self.fd_map.get(&user_data) {
                self.add_send(fd, buffer, res as usize)?;
            } else {
                eprintln!("Error: FD not found for read operation");

                // Deallocate the buffer
                unsafe {
                    let _ = Box::from_raw(buffer);
                }
            }
        } else if res == 0 {
            println!("Connection closed");
            unsafe {
                let _ = Box::from_raw(buffer);
            } // Deallocate the buffer
            self.fd_map.remove(&user_data);
        } else {
            eprintln!("Read failed with error: {}", -res);
            unsafe {
                let _ = Box::from_raw(buffer);
            } // Deallocate the buffer
            self.fd_map.remove(&user_data);
        }

        Ok(())
    }
}
