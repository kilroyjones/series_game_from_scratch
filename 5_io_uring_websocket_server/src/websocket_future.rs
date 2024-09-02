use std::fmt;
use std::future::Future;
use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::pin::Pin;
use std::str;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use crate::base64::Base64;
use crate::sha1::Sha1;

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

pub enum WebSocketState {
    Handshake,
    Connected,
    Closed,
}

pub struct WebSocketFuture {
    pub stream: TcpStream,
    state: WebSocketState,
    pub read_buffer: Vec<u8>,
    pub write_buffer: Vec<u8>,
    last_ping: Instant,
    pong_received: bool,
}

impl WebSocketFuture {
    pub fn new(stream: TcpStream) -> Self {
        WebSocketFuture {
            stream,
            state: WebSocketState::Handshake,
            read_buffer: vec![0; 2048],
            write_buffer: Vec::new(),
            last_ping: Instant::now(),
            pong_received: false,
        }
    }

    fn handle_handshake_state(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), WebSocketError>> {
        let bytes_read = match self.stream.read(&mut self.read_buffer) {
            Ok(0) => return Poll::Ready(Ok(())),
            Ok(n) => n,
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                cx.waker().wake_by_ref();
                return Poll::Pending;
            }
            Err(e) => return Poll::Ready(Err(WebSocketError::IoError(e))),
        };

        let request = str::from_utf8(&self.read_buffer[..bytes_read])?;
        if !request.starts_with("GET") {
            return Poll::Ready(Err(WebSocketError::NonGetRequest));
        }

        let request_owned = request.to_string();

        let response = self.process_handshake(&request_owned)?;

        self.write_buffer.extend_from_slice(response.as_bytes());
        self.state = WebSocketState::Connected;
        self.send_ping()?;

        Poll::Pending
    }

    fn process_handshake(&mut self, request: &str) -> Result<String, WebSocketError> {
        let mut base64 = Base64::new();
        let mut sha1 = Sha1::new();

        let key_header = "Sec-WebSocket-Key: ";
        let key = request
            .lines()
            .find(|line| line.starts_with(key_header))
            .map(|line| line[key_header.len()..].trim())
            .ok_or_else(|| {
                WebSocketError::HandshakeError(
                    "Could not find Sec-WebSocket-Key in HTTP request header".to_string(),
                )
            })?;

        let response_key = format!("{}258EAFA5-E914-47DA-95CA-C5AB0DC85B11", key);
        let hash = sha1.hash(response_key).map_err(|_| {
            WebSocketError::HandshakeError("Failed to hash the response key".to_string())
        })?;

        let header_key = base64.encode(hash).map_err(|_| {
            WebSocketError::HandshakeError("Failed to encode the hash as Base64".to_string())
        })?;

        Ok(format!(
            "HTTP/1.1 101 Switching Protocols\r\n\
            Upgrade: websocket\r\n\
            Connection: Upgrade\r\n\
            Sec-WebSocket-Accept: {}\r\n\r\n",
            header_key
        ))
    }

    fn handle_write_buffer(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), WebSocketError>> {
        if !self.write_buffer.is_empty() {
            match self.stream.write(&self.write_buffer) {
                Ok(0) => return Poll::Ready(Ok(())),
                Ok(n) => {
                    self.write_buffer.drain(..n);
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                    cx.waker().wake_by_ref();
                    return Poll::Pending;
                }
                Err(e) => return Poll::Ready(Err(WebSocketError::IoError(e))),
            }
        }
        Poll::Pending
    }

    fn handle_read_buffer(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), WebSocketError>> {
        let bytes_read = match self.stream.read(&mut self.read_buffer) {
            Ok(0) => return Poll::Ready(Ok(())),
            Ok(n) => n,
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                cx.waker().wake_by_ref();
                return Poll::Pending;
            }
            Err(e) => return Poll::Ready(Err(WebSocketError::IoError(e))),
        };

        if bytes_read > 0 {
            let frame_data = self.read_buffer[..bytes_read].to_vec();
            self.process_frame(&frame_data)?;
        }

        Poll::Pending
    }

    pub fn process_frame(&mut self, buffer: &[u8]) -> Result<(), WebSocketError> {
        match self.parse_frame(buffer)? {
            Frame::Pong => {
                self.pong_received = true;
            }
            Frame::Ping => {
                self.send_pong()?;
            }
            Frame::Close => {
                self.state = WebSocketState::Closed;
            }
            Frame::Text(data) => {
                if let Ok(text) = String::from_utf8(data) {
                    self.send_text(&text)?;
                }
            }
            Frame::Binary(_) => {}
        }
        Ok(())
    }

    fn handle_connected_state(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), WebSocketError>> {
        self.handle_write_buffer(cx)?;
        self.handle_read_buffer(cx)?;
        self.check_ping_timeout()?;

        Poll::Pending
    }

    fn send_pong(&mut self) -> Result<(), WebSocketError> {
        self.write_buffer.extend_from_slice(&[0x8A, 0x00]);
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

    fn check_ping_timeout(&mut self) -> Result<(), WebSocketError> {
        if self.last_ping.elapsed() > Duration::from_secs(10) {
            if !self.pong_received {
                return Err(WebSocketError::HandshakeError(
                    "Pong not received".to_string(),
                ));
            }
            self.send_ping()?;
            self.pong_received = false;
            self.last_ping = Instant::now();
        }
        Ok(())
    }

    fn send_ping(&mut self) -> io::Result<usize> {
        self.write_buffer.extend_from_slice(&[0x89, 0x00]);
        Ok(2)
    }

    pub fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.stream.read(buf)
    }

    pub fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.stream.write(buf)
    }

    fn send_text(&mut self, data: &str) -> Result<(), WebSocketError> {
        let mut frame = Vec::new();
        frame.push(0x81);

        let data_bytes = data.as_bytes();
        let length = data_bytes.len();

        if length <= 125 {
            frame.push(length as u8);
        } else if length <= 65535 {
            frame.push(126);
            frame.extend_from_slice(&(length as u16).to_be_bytes());
        } else {
            frame.push(127);
            frame.extend_from_slice(&(length as u64).to_be_bytes());
        }

        frame.extend_from_slice(data_bytes);
        self.write_buffer.extend_from_slice(&frame);
        Ok(())
    }
}

impl Future for WebSocketFuture {
    type Output = Result<(), WebSocketError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.state {
            WebSocketState::Handshake => self.handle_handshake_state(cx),
            WebSocketState::Connected => self.handle_connected_state(cx),
            WebSocketState::Closed => Poll::Ready(Ok(())),
        }
    }
}
