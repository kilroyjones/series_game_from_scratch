mod entry;
mod iouring;
mod server;

use std::io;

use crate::server::EchoServer;

#[allow(non_upper_case_globals)]
#[allow(non_camel_case_types)]
#[allow(non_snake_case)]
#[allow(dead_code)]
mod bindings {
    #[cfg(not(rust_analyzer))]
    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}

fn main() -> io::Result<()> {
    let mut server = EchoServer::new(8080)?;
    println!("Echo server listening on port 8080");
    server.run()
}