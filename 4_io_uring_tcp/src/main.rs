mod base64;
mod sha1;
use crate::base64::Base64;
use crate::sha1::Sha1;

use std::alloc::{alloc, dealloc};
use std::collections::HashMap;
use std::io::{self, repeat, Write};
use std::mem::{size_of, zeroed};
use std::net::TcpListener;
use std::os::fd::FromRawFd;
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

use self::bindings::*;

mod websocket;

use websocket::{Frame, WebSocket, WebSocketError};

const QUEUE_DEPTH: u32 = 256;
const BUFFER_SIZE: usize = 2048;

struct UringWebSocketServer {
    ring: io_uring,
    listener: TcpListener,
    fd_map: HashMap<u64, RawFd>,
    websockets: HashMap<RawFd, WebSocket>,
}

// macro_rules! syscall {
//     ($nr:ident $(, $arg:expr)*) => {{
//         let res: isize;
//         unsafe {
//             asm!(
//                 "syscall",
//                 in("rax") $nr as usize,
//                 $(in("rdi") $arg,)*
//                 out("rcx") _,
//                 out("r11") _,
//                 lateout("rax") res,
//             );
//         }
//         res
//     }};
// }

// fn close(fd: RawFd) -> io::Result<()> {
//     let ret = unsafe { syscall!(CLOSE, fd) };
//     if ret == 0 {
//         Ok(())
//     } else {
//         Err(io::Error::last_os_error())
//     }
// }

impl UringWebSocketServer {
    fn new(port: u16) -> io::Result<Self> {
        let listener = TcpListener::bind(("0.0.0.0", port))?;
        let mut ring: io_uring = unsafe { std::mem::zeroed() };

        let ret = unsafe { io_uring_queue_init(QUEUE_DEPTH, &mut ring, 0) };
        if ret < 0 {
            return Err(io::Error::from_raw_os_error(-ret));
        }

        Ok(Self {
            ring,
            listener,
            fd_map: HashMap::new(),
            websockets: HashMap::new(),
        })
    }

    fn run_event_loop(&mut self) -> io::Result<()> {
        self.add_accept()?;

        loop {
            let ret = unsafe { io_uring_submit_and_wait(&mut self.ring, 1) };
            if ret < 0 {
                return Err(io::Error::from_raw_os_error(-ret));
            }

            self.process_completions()?;
        }
    }

    fn add_accept(&mut self) -> io::Result<()> {
        let sqe = unsafe { io_uring_get_sqe(&mut self.ring) };
        if sqe.is_null() {
            return Err(io::Error::new(io::ErrorKind::Other, "Failed to get SQE"));
        }

        unsafe {
            io_uring_prep_accept(
                sqe,
                self.listener.as_raw_fd(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
            );
            (*sqe).user_data = 0; // Use 0 as a marker for accept operations
        }

        Ok(())
    }

    fn add_read(&mut self, fd: RawFd) -> io::Result<()> {
        let sqe = unsafe { io_uring_get_sqe(&mut self.ring) };
        if sqe.is_null() {
            return Err(io::Error::new(io::ErrorKind::Other, "Failed to get SQE"));
        }

        let buffer = Box::into_raw(Box::new([0u8; BUFFER_SIZE]));
        let user_data = buffer as u64;

        unsafe {
            io_uring_prep_read(
                sqe,
                fd,
                buffer as *mut std::ffi::c_void,
                BUFFER_SIZE as u32,
                0,
            );
            (*sqe).user_data = user_data;
        }

        self.fd_map.insert(user_data, fd);

        Ok(())
    }

    fn add_write(&mut self, fd: RawFd, buffer: *mut u8, len: usize) -> io::Result<()> {
        let sqe = unsafe { io_uring_get_sqe(&mut self.ring) };
        if sqe.is_null() {
            return Err(io::Error::new(io::ErrorKind::Other, "Failed to get SQE"));
        }

        let user_data = buffer as u64 | (1 << 63); // Use high bit to mark write operations

        unsafe {
            io_uring_prep_write(sqe, fd, buffer as *const std::ffi::c_void, len as u32, 0);
            (*sqe).user_data = user_data;
        }

        self.fd_map.insert(user_data, fd);

        Ok(())
    }

    fn process_completions(&mut self) -> io::Result<()> {
        let mut cqe: *mut io_uring_cqe = std::ptr::null_mut();

        unsafe {
            io_uring_peek_cqe(&mut self.ring, &mut cqe);
        }

        while !cqe.is_null() {
            let user_data = unsafe { (*cqe).user_data };
            let res = unsafe { (*cqe).res };

            if user_data == 0 {
                // Accept operation
                if res >= 0 {
                    println!("Accepted new connection: {}", res);
                    let stream = unsafe { std::net::TcpStream::from_raw_fd(res) };
                    let mut websocket = WebSocket::new(stream);
                    match websocket.connect() {
                        Ok(_) => {
                            println!("WebSocket handshake successful");
                            self.websockets.insert(res, websocket);
                            self.add_read(res)?;
                        }
                        Err(e) => {
                            println!("WebSocket handshake failed: {:?}", e);
                            drop(unsafe { std::net::TcpStream::from_raw_fd(res) });
                        }
                    }
                } else {
                    println!("Accept failed with error: {}", -res);
                }
                self.add_accept()?; // Queue up another accept
            } else if user_data & (1 << 63) != 0 {
                // Write operation
                let buffer = (user_data & !(1 << 63)) as *mut u8;
                if res >= 0 {
                    println!("Write completed: {} bytes", res);
                    if let Some(&fd) = self.fd_map.get(&(user_data & !(1 << 63))) {
                        self.add_read(fd)?; // Queue up another read after successful write
                    }
                } else {
                    println!("Write failed with error: {}", -res);
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
                        if let Some(websocket) = self.websockets.get_mut(&fd) {
                            let data = unsafe { std::slice::from_raw_parts(buffer, res as usize) };
                            match websocket.parse_frame(data) {
                                Ok(Frame::Text(text)) => {
                                    println!("Received text: {:?}", String::from_utf8_lossy(&text));
                                    // Echo the text back
                                    if let Err(e) =
                                        websocket.send_text(&String::from_utf8_lossy(&text))
                                    {
                                        println!("Failed to send echo: {:?}", e);
                                    }
                                }
                                Ok(Frame::Close) => {
                                    println!("Received close frame");
                                    // Handle WebSocket close
                                    if let Some(websocket) = self.websockets.remove(&fd) {
                                        drop(websocket);
                                    }
                                }
                                Ok(_) => {
                                    // Handle other frame types if necessary
                                }
                                Err(e) => {
                                    println!("Error parsing WebSocket frame: {:?}", e);
                                }
                            }
                        }
                        self.add_read(fd)?; // Queue up another read
                    } else {
                        println!("Error: FD not found for read operation");
                    }
                } else if res == 0 {
                    println!("Connection closed");
                    if let Some(&fd) = self.fd_map.get(&user_data) {
                        if let Some(websocket) = self.websockets.remove(&fd) {
                            drop(websocket);
                        }
                    }
                } else {
                    println!("Read failed with error: {}", -res);
                }
                unsafe {
                    Box::from_raw(buffer);
                } // Deallocate the buffer
                self.fd_map.remove(&user_data);
            }

            unsafe {
                io_uring_cqe_seen(&mut self.ring, cqe);
                io_uring_peek_cqe(&mut self.ring, &mut cqe);
            }
        }

        Ok(())
    }
}

fn main() -> io::Result<()> {
    let mut server = UringWebSocketServer::new(8080)?;
    println!("WebSocket server listening on port 8080");
    server.run_event_loop()
}
