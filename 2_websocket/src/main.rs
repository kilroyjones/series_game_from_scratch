mod base64;
mod sha1;
mod websocket;

use std::net::{TcpListener, TcpStream};
use std::thread;

use websocket::WebSocket;

/// Handles a connection using our websockets
///
/// We create a new WebSocket instance, pass it the stream and then connect.
///
fn handle_client(stream: TcpStream) {
    let mut ws = WebSocket::new(stream);

    match ws.connect() {
        Ok(()) => {
            println!("WebSocket connection established");
            match ws.handle_connection() {
                Ok(_) => {
                    println!("Connection ended without error");
                }
                Err(e) => {
                    println!("Connection ended with error {:?}", e);
                }
            }
        }
        Err(e) => {
            println!("Failed to establish a WebSocket connection: {}", e);
        }
    }
}

/// Listens for incoming connections
///
/// We listen to incoming connections and create new threads for each one of them.
///
fn main() {
    //
    let listener = TcpListener::bind("127.0.0.1:8080").expect("Could not bind to port");
    println!("WebSocket server is running on ws://127.0.0.1:8080/");

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                thread::spawn(move || {
                    handle_client(stream);
                });
            }
            Err(e) => {
                println!("Failed to accept client: {}", e);
            }
        }
    }
}
