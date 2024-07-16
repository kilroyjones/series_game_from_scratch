//! Websocket
//!
//! This is a "from scratch" websocket implementation in that it uses onlhy the
//! Rust standard library. This is a minimal implementation is meant as a
//! learning tool only.
//!

use crate::base64::Base64;
use crate::sha1::Sha1;

use std::fmt;
use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::str;
use std::time::Duration;

/// Frame
///
/// Denotes the types of websocket frames we'll be working with. Frames are a
/// "header + data" and that data could be binary or text as denoted by "Data"
/// below. Alternatively, it could frame for a ping, pong or to close a the
/// socket (the shortest of frames)
///
#[derive(Debug)]
pub enum Frame {
    Text(Vec<u8>),
    Binary(Vec<u8>),
    Ping,
    Pong,
    Close,
}

/// WebSocketError
///
/// These are our custom error messages.
///
///     HandshakeError: Provides errors during the initial connection process.
///
///     IoError: Primarily details with errors that occur during sending and
///     receiving messages.
///
///     NonGetRequest: A one-off request used upon connection.
///
///     ProtocolError: When parsing the frame these messages will occur if the
///     frame is malformed.
///
///     Utf8Error: Used when checking incoming data.  
///
#[derive(Debug)]
pub enum WebSocketError {
    HandshakeError(String),
    IoError(io::Error),
    NonGetRequest,
    ProtocolError(String),
    Utf8Error(str::Utf8Error),
}

/// WebSocketError Display implementation
///
/// These are wrappers for writing our error messages out.
///
impl fmt::Display for WebSocketError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            WebSocketError::HandshakeError(ref msg) => write!(f, "Handshake error: {}", msg),
            WebSocketError::IoError(ref err) => write!(f, "I/O error: {}", err),
            WebSocketError::NonGetRequest => write!(f, "Received non-GET request"),
            WebSocketError::ProtocolError(ref msg) => write!(f, "Protocol error: {}", msg),
            WebSocketError::Utf8Error(ref err) => write!(f, "UTF-8 decoding error: {}", err),
        }
    }
}

/// Allows for automatic conversion from io:Error to WebSocketError
///
impl From<io::Error> for WebSocketError {
    fn from(err: io::Error) -> WebSocketError {
        WebSocketError::IoError(err)
    }
}

/// Allows for automatic conversion from str::Utf8Error to WebSocketError
///
impl From<str::Utf8Error> for WebSocketError {
    fn from(err: str::Utf8Error) -> WebSocketError {
        WebSocketError::Utf8Error(err)
    }
}

/// Defines the WebSocket
///
/// For now the WebSocket is only composed of a TcpStream, but normally we'd
/// want to attach other information about the connection to it.
///
pub struct WebSocket {
    stream: TcpStream,
}

impl WebSocket {
    /// Creates the WebSocket instance
    ///
    pub fn new(stream: TcpStream) -> WebSocket {
        WebSocket { stream }
    }

    /// Connect the websocket
    ///
    /// This will read in the HTTP request and check if it's a GET or not. It will then
    /// call the handle_handshake function which parses the request header.
    ///
    pub fn connect(&mut self) -> Result<(), WebSocketError> {
        let mut buffer: [u8; 1024] = [0; 1024];

        // From the stream read in the HTTP request
        let byte_length = match self.stream.read(&mut buffer) {
            Ok(bytes) => bytes,
            Err(e) => return Err(WebSocketError::IoError(e)),
        };

        // Read only the request from the buffer
        let request = str::from_utf8(&buffer[..byte_length])?;

        // We only want to deal with GET requests for the upgrade
        if request.starts_with("GET") == false {
            return Err(WebSocketError::NonGetRequest);
        }

        // Get the HTTP response header and send it back
        let response = self.handle_handshake(request)?;
        self.stream
            .write_all(response.as_bytes())
            .map_err(WebSocketError::IoError)?;

        self.stream.flush().map_err(WebSocketError::IoError)?;
        Ok(())
    }

    /// Validate the websocket upgrade request
    ///
    /// Checks that the Sec-WebSocket-Key exists and then formulates a response
    /// key, hashing it using sha-1 and then encoding with base64. There is a hardcoded
    /// HTTP response attached to the header to upgrade the connection to websockets.
    ///
    fn handle_handshake(&mut self, request: &str) -> Result<String, WebSocketError> {
        let mut base64 = Base64::new();
        let mut sha1 = Sha1::new();

        let key_header = "Sec-WebSocket-Key: ";

        // Given the request we find the line starting the the key_header and then find the
        // key sent from the client.
        let key = request
            .lines()
            .find(|line| line.starts_with(key_header))
            .map(|line| line[key_header.len()..].trim())
            .ok_or_else(|| {
                WebSocketError::HandshakeError(
                    "Could not find Sec-WebSocket-Key in HTTP request header".to_string(),
                )
            })?;

        // Append key with the necessary id as per the WebSocket Protocol specification
        let response_key = format!("{}258EAFA5-E914-47DA-95CA-C5AB0DC85B11", key);

        // First we take the hash of the random key sent by the client
        let hash = sha1.hash(response_key).map_err(|_| {
            WebSocketError::HandshakeError("Failed to hash the response key".to_string())
        })?;

        // Second we encode that hash as Base64
        let header_key = base64.encode(hash).map_err(|_| {
            WebSocketError::HandshakeError("Failed to encode the hash as Base64".to_string())
        })?;

        // Lastly we attach that key to the our response header
        Ok(format!(
            "HTTP/1.1 101 Switching Protocols\r\n\
            Upgrade: websocket\r\n\
            Connection: Upgrade\r\n\
            Sec-WebSocket-Accept: {}\r\n\r\n",
            header_key
        ))
    }

    /// Handles the connection
    ///
    /// This is a loop which will continue until either the connection is
    /// terminated (Frame::Close) or a connection timeout which is currently
    /// hardcoded as 5 seconds.
    ///
    /// Currently it handles PING, PONG, CLOSE and TEXT or BINARY data.
    ///
    /// Note: Later I will move this functionality outside of websocket.rs.
    ///  
    pub fn handle_connection(&mut self) -> Result<(), WebSocketError> {
        // A buffer of 2048 should be large enough to handle incoming data.
        let mut buffer = [0; 2048];

        // Send initial ping
        self.send_ping()?;
        let mut last_ping = std::time::Instant::now();
        let mut pong_received = false;

        // Primary loop which runs inside the thread spawned in main.rs
        loop {
            // This is the check to see if the connection has timed out or not.
            // We've hardcoded it to a default of 10 seconds, but it would be
            // good have this configurable later on.
            if last_ping.elapsed() > Duration::from_secs(10) {
                if pong_received == false {
                    println!("Pong not received; disconnecting client.");
                    break;
                }

                if let Err(_) = self.send_ping() {
                    println!("Ping failed; disconnecting client.");
                    break;
                }

                pong_received = false;
                last_ping = std::time::Instant::now();
            }

            // Read in the current stream or data.
            match self.stream.read(&mut buffer) {
                // read(&mut buffer) will return a usize, and we'll want to process that if and only
                // if it's larger than 0. We then parse the frame in the parse_frame function.
                Ok(n) if n > 0 => match self.parse_frame(&buffer[..n]) {
                    Ok(Frame::Pong) => {
                        println!("Pong received");
                        pong_received = true;
                        continue;
                    }

                    Ok(Frame::Ping) => {
                        if self.send_pong().is_err() {
                            println!("Failed to send pong");
                            break;
                        }
                    }

                    Ok(Frame::Close) => {
                        println!("Client initiated close");
                        break;
                    }

                    Ok(Frame::Text(data)) => match String::from_utf8(data) {
                        Ok(valid_text) => {
                            println!("Received data: {}", valid_text);
                            if self.send_text(&valid_text).is_err() {
                                println!("Failed to send echo message");
                                break;
                            }
                        }
                        Err(utf8_err) => {
                            return Err(WebSocketError::Utf8Error(utf8_err.utf8_error()));
                        }
                    },

                    // We are not going to handle this binary data at this point.
                    Ok(Frame::Binary(data)) => {
                        println!("Binary data received: {:?}", data);
                        continue;
                    }

                    Err(e) => {
                        println!("Error parsing frame: {}", e);
                        break;
                    }
                },
                Ok(_) => {}
                // If there's an error, end the connection
                Err(e) => {
                    println!("Error reading from stream: {}", e);
                    break;
                }
            }
        }
        Ok(())
    }

    /// Parses in incoming frame
    ///
    /// This function goes through the following steps:
    ///     1. Validates length
    ///     2. Checks if frame is masked
    ///     3. Checks extended payload length
    ///     4. Decodes the using XOR with the mask
    ///     5. Returns an Ok with opcode and data (if exists)
    ///
    fn parse_frame(&mut self, buffer: &[u8]) -> Result<Frame, WebSocketError> {
        // The smallest length it can be is two bytes for a Close frame
        if buffer.len() < 2 {
            return Err(WebSocketError::ProtocolError("Frame too short".to_string()));
        }

        let first_byte = buffer[0];

        // This is not needed for this demo, but will need to implemented later on in
        // order to deal with fragmented message. If, for example we'd like to send
        // messages which exceed the buffer length defined in handle_connection, then
        // this will be needed to detect and act accordingly.
        // let fin = (first_byte & 0x80) != 0;

        let opcode = first_byte & 0x0F; // Determines opcode

        // Extract the mask
        let second_byte = buffer[1];
        let masked = (second_byte & 0x80) != 0;

        // Determine payload length by getting the last 7 bits. If they are set
        // to 126, then it will include the next 16 bits, providing a maximum of
        // 65535 bytes.
        let mut payload_len = (second_byte & 0x7F) as usize;

        // If no masks exists, bail
        if masked == false {
            return Err(WebSocketError::ProtocolError(
                "Frames from client must be masked".to_string(),
            ));
        }

        // Set initially to 2 so that we skip over the first and second byte as
        // used above.
        let mut offset = 2;

        if payload_len == 126 {
            // If the payload has been noted as an extended payload, but the buffer
            // content is not long enough then throw an error.
            if buffer.len() < 4 {
                return Err(WebSocketError::ProtocolError(
                    "Frame too short for extended payload length".to_string(),
                ));
            }

            // Get the current payload length and advance two bytes since the
            // length of the payload will be contained in those two bytes.
            payload_len = u16::from_be_bytes([buffer[offset], buffer[offset + 1]]) as usize;
            offset += 2;
        } else if payload_len == 127 {
            // We will ignore extra large payload lengths for now. This would be
            // payloads that are 2^64, or a size denoted by 4 bytes.
            return Err(WebSocketError::ProtocolError(
                "Extended payload length too large".to_string(),
            ));
        }

        // Given this, we have the initial two bytes + the offset from the
        // payload length and then finally the next 4 bytes are the masking key.
        // Here we check if payload length plus all that actually exists or not.
        // If the overall buffer is shorter then error out.
        if buffer.len() < offset + 4 + payload_len {
            return Err(WebSocketError::ProtocolError(
                "Frame too short for mask and data".to_string(),
            ));
        }

        // Extract the masking key
        let mask = &buffer[offset..offset + 4];

        // Advance past the masking key and start on the data
        offset += 4;

        // Extract and apply the masking key via XOR
        let mut data = Vec::with_capacity(payload_len);
        for i in 0..payload_len {
            data.push(buffer[offset + i] ^ mask[i % 4]);
        }

        // Return the opcode and data if given
        Ok(match opcode {
            0x01 => Frame::Text(data),   // text frame
            0x02 => Frame::Binary(data), // binary frame
            0x08 => Frame::Close,        // close frame
            0x09 => Frame::Ping,         // ping frame
            0x0A => Frame::Pong,         // pong frame
            _ => return Err(WebSocketError::ProtocolError("Unknown opcode".to_string())),
        })
    }

    /// Sends a ping
    ///
    /// 0x89 is made of 0x80, indicating FIN bit set and it's the end of the
    /// message, as well as 0x09, which indicates it's a ping. The 0x00 is no
    /// data being sent.
    ///
    fn send_ping(&mut self) -> io::Result<usize> {
        println!("Ping sent");
        self.stream.write(&[0x89, 0x00])
    }

    /// Sends a pong
    ///
    /// 0x8A is made of 0x80, indicating FIN bit set and it's the end of the
    /// message, as well as 0x0A, which indicates it's a pong. The 0x00 is no
    /// data being sent.
    ///
    fn send_pong(&mut self) -> Result<(), WebSocketError> {
        println!("Pong sent");
        self.stream.write(&[0x8A, 0x00])?;
        Ok(()) // Opcode for pong is 0xA and FIN set
    }

    /// Sends text
    ///
    /// Creates a frame and then sends through the current TcpStream.
    ///
    fn send_text(&mut self, data: &str) -> Result<(), WebSocketError> {
        let mut frame = Vec::new();

        // FIN bit and code 0x01 for text data
        frame.push(0x81);

        let data_bytes = data.as_bytes();
        let length = data_bytes.len();

        // These sets payload length information within the initial bytes
        if length <= 125 {
            frame.push(length as u8); // Payload length fits in one byte
        } else if length <= 65535 {
            frame.push(126); // Signal that the next two bytes contain the payload length
            frame.extend_from_slice(&(length as u16).to_be_bytes());
        } else {
            frame.push(127); // Signal that the next eight bytes contain the payload length
            frame.extend_from_slice(&(length as u64).to_be_bytes());
        }

        // Append the data itself as bytes.
        frame.extend_from_slice(data_bytes);

        self.stream.write_all(&frame)?;
        self.stream.flush()?;
        Ok(())
    }
}
