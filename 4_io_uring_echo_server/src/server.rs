/// Echo server
///
/// This echo server is based on on bindings to the Linux liburing library (see
/// build.rs). It will only work if the liburing library has been installed.
///
use crate::bindings::*;
use crate::entry::Entry;
use crate::iouring::IoUring;
use std::collections::HashMap;
use std::io;
use std::net::TcpListener;
use std::os::unix::io::{AsRawFd, RawFd};
use std::ptr;
use std::time::Duration;

const QUEUE_DEPTH: u32 = 256;
const BUFFER_SIZE: usize = 1024;

/// Operation types
///
/// This defines the operation types we'll be using. This setup leaves it open
/// to easily adding more. Note, both Read and Write store the location of the
/// operation's associated buffer.
///
enum Operation {
    Accept,
    Read(*mut u8),
    Write(*mut u8),
}

/// Operation data
///
/// This will be part of a key-value pair, as the value, which holds the
/// operation information and the associated file descriptor of the socket.
///
struct OperationData {
    op: Operation,
    fd: RawFd,
}

/// Echo serer
///
/// Holds the ring, the primary TcpListener (this could alternatively be
/// represented by a file descriptor, but this makes it easier). Lastly, we have
/// our operations look up which uses a unique u64 id for each queue entry that
/// is match to our operation data.
///
pub struct EchoServer {
    ring: IoUring,
    listener: TcpListener,
    operations: HashMap<u64, OperationData>,
    next_id: u64,
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
            operations: HashMap::new(),
            next_id: 0,
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
        self.add_accept()?;
        self.ring.submit()?;

        loop {
            match self.ring.peek_completion() {
                Some(cqe) => self.handle_completion(cqe)?,
                None => {
                    self.ring.submit()?;
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
        }
    }

    /// Accept connections
    ///
    /// We create an accept empty accept entry and then add the listener's file
    /// descriptor. We set the parameters for addr and addrlen to null since we
    /// don't care about the IP address for now. Later, we'll want to grab these.
    ///
    fn add_accept(&mut self) -> io::Result<()> {
        let mut entry = Entry::new();
        entry
            .set_accept(self.listener.as_raw_fd(), ptr::null_mut(), ptr::null_mut())
            .set_user_data(self.generate_entry_id(Operation::Accept, self.listener.as_raw_fd()));
        self.ring.create_entry(&entry)
    }

    /// Receive information
    ///
    /// We create a buffer to store the incoming information and a recv entry
    /// and then use the unique memory address of that buffer as the key in our
    /// hash with the value as the fd.
    ///
    fn add_recv(&mut self, fd: RawFd) -> io::Result<()> {
        let buffer = Box::into_raw(Box::new([0u8; BUFFER_SIZE])) as *mut u8;
        let user_data = self.generate_entry_id(Operation::Read(buffer), fd);

        let mut entry = Entry::new();
        entry
            .set_recv(fd, buffer as *mut u8, BUFFER_SIZE, 0)
            .set_user_data(user_data);

        self.ring.create_entry(&entry)
    }

    /// Send information
    ///
    /// When sending we create a unique id, which we'll store in the user_data
    /// portion of the iouring submission queue entry. That entry is created in
    /// the shared memory of the queue that exists between user and kernel
    /// space.
    ///
    fn add_send(&mut self, fd: RawFd, buffer: *mut u8, len: usize) -> io::Result<()> {
        let user_data = self.generate_entry_id(Operation::Write(buffer), fd);

        let mut entry = Entry::new();
        entry
            .set_send(fd, buffer as *const u8, len, 0)
            .set_user_data(user_data);

        self.ring.create_entry(&entry)
    }

    /// Creates entry id
    ///
    /// This is needed because when we create an entry, say for reading from a
    /// user, we'll need to know the associated file descriptor (socket) to echo
    /// an answer to. It's a way to match submission and completition queue
    /// entries with the given file descriptor.
    ///
    fn generate_entry_id(&mut self, op: Operation, fd: RawFd) -> u64 {
        let user_data = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        self.operations.insert(user_data, OperationData { op, fd });
        user_data
    }

    /// Handles completed queue entries
    ///
    /// Grade the user_data from our completion queue entry (cqe) and then remove it
    /// from our operations hashmap. Each operation has a variant and associated file
    /// description AND possibly buffer (Read/Write). We then pass those along to the
    /// respective handler.
    ///
    fn handle_completion(&mut self, cqe: io_uring_cqe) -> io::Result<()> {
        let user_data = cqe.user_data;
        let res = cqe.res; // This indicates the succces or failure or the operation.

        if let Some(op_data) = self.operations.remove(&user_data) {
            match op_data.op {
                Operation::Accept => self.handle_accept(res)?,
                Operation::Read(buffer) => self.handle_read(res, buffer, op_data.fd)?,
                Operation::Write(buffer) => self.handle_write(res, buffer, op_data.fd)?,
            }
        }

        Ok(())
    }

    /// Handle Accept
    ///
    /// We check the result to see if a connection is being made, if so we queue
    /// of a receive. If result is negative, then queue may be full. No matter
    /// what happens we queue up another accept, which keeps us listening for
    /// more connections.
    ///
    fn handle_accept(&mut self, res: i32) -> io::Result<()> {
        if res >= 0 {
            println!("Accepted new connection: {}", res);
            self.add_recv(res)?;
        } else if res == -(EAGAIN as i32) {
            println!("No new connection available");
        } else {
            eprintln!("Accept failed with error: {}", -res);
        }

        self.add_accept()
    }

    /// Handle read
    ///
    /// If we get a successful read we convert the buffer to a readable string,
    /// otherwise if we get 0 the connection is closed and we release the
    /// buffer.
    ///
    /// Releasing the buffer is a bit odd. We take it, wrap it in a box so that
    /// Rust will be able to clean it up after it does out of scope. We do this
    /// on connection closed or failure.
    ///
    fn handle_read(&mut self, res: i32, buffer: *mut u8, fd: RawFd) -> io::Result<()> {
        if res > 0 {
            let slice = unsafe { std::slice::from_raw_parts(buffer, res as usize) };
            let text = String::from_utf8_lossy(slice);
            println!("Read {} bytes: {}", res, text);

            self.add_send(fd, buffer, res as usize)?;
        } else if res == 0 {
            println!("Connection closed");
            unsafe {
                let _ = Box::from_raw(buffer);
            }
        } else {
            eprintln!("Read failed with error: {}", -res);
            unsafe {
                let _ = Box::from_raw(buffer);
            }
        }

        Ok(())
    }

    /// Handle write
    ///
    /// We write and then queue another receive on the same socket. No matter
    /// what, we release the buffer used to store the information.
    ///
    fn handle_write(&mut self, res: i32, buffer: *mut u8, fd: RawFd) -> io::Result<()> {
        if res >= 0 {
            println!("Send completed: {} bytes", res);
            self.add_recv(fd)?;
        } else {
            eprintln!("Write failed with error: {}", -res);
        }

        unsafe {
            let _ = Box::from_raw(buffer);
        }

        Ok(())
    }
}
