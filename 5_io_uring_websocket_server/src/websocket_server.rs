use crate::iouring::IoUring;
use crate::websocket_future::WebSocketFuture;
use std::collections::HashMap;
use std::future::Future;
use std::io;
use std::io::Read;
use std::io::Write;
use std::net::TcpListener;
use std::net::TcpStream;
use std::os::fd::AsRawFd;
use std::os::fd::FromRawFd;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Wake, Waker};

const BUFFER_SIZE: usize = 4096;

pub struct WebSocketServer {
    listener: TcpListener,
    uring: IoUring,
    connections: HashMap<u64, WebSocketFuture>,
    next_connection_id: u64,
}

impl WebSocketServer {
    pub fn new(addr: &str, entries: u32) -> io::Result<Self> {
        let listener = TcpListener::bind(addr)?;
        listener.set_nonblocking(true)?;
        let uring = IoUring::new(entries)?;
        Ok(Self {
            listener,
            uring,
            connections: HashMap::new(),
            next_connection_id: 1,
        })
    }

    pub fn run(&mut self) -> io::Result<()> {
        println!(
            "WebSocket server listening on {}",
            self.listener.local_addr()?
        );

        let noop_waker = Arc::new(NoopWaker);
        let waker = Waker::from(noop_waker);
        let mut context = Context::from_waker(&waker);

        // Set up listener for accepting new connections
        let mut entry = self.uring.create_entry();
        entry.set_accept(
            self.listener.as_raw_fd(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
        );
        self.uring.submit()?;

        loop {
            // match self.uring.peek_completion() {
            //     Some(cqe) => self.handle_completion(cqe)?,
            //     None => {
            //         self.uring.submit()?;
            //         std::thread::sleep(Duration::from_millis(1));
            //     }
            // }
        }
    }
}

// Thread-safe no-op waker implementation
struct NoopWaker;

impl Wake for NoopWaker {
    fn wake(self: Arc<Self>) {
        // Do nothing
    }
}
