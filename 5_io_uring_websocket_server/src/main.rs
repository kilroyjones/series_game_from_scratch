#[allow(non_upper_case_globals)]
#[allow(non_camel_case_types)]
#[allow(non_snake_case)]
#[allow(dead_code)]
mod bindings {
    #[cfg(not(rust_analyzer))]
    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}
mod base64;
mod entry;
mod iouring;
mod sha1;
mod websocket_future;
mod websocket_server;
use std::io;

use websocket_server::WebSocketServer;

fn main() -> std::io::Result<()> {
    let addr = "127.0.0.1:8080";
    let entries = 256; // Number of entries for the io_uring queue

    let mut server = WebSocketServer::new(addr, entries)?;
    server.run()
}
