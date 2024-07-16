use std::string::FromUtf8Error;

const BASE64_CHARSET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub struct Base64;

#[derive(Debug)]
pub enum Base64Error {
    InvalidCharacter,
    Utf8Error(FromUtf8Error),
}

impl std::fmt::Display for Base64Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match *self {
            Base64Error::InvalidCharacter => write!(f, "Invalid character in input"),
            Base64Error::Utf8Error(ref e) => e.fmt(f),
        }
    }
}

impl Base64 {
    pub fn new() -> Self {
        Base64 {}
    }

    pub fn encode(&mut self, input: &str) -> Result<String, Base64Error> {
        let mut encoded = Vec::new();
        let bytes = input.as_bytes();
        let mut buffer: u32;

        for chunk in bytes.chunks(3) {
            buffer = match chunk.len() {
                3 => (u32::from(chunk[0]) << 16) | (u32::from(chunk[1]) << 8) | u32::from(chunk[2]),
                2 => (u32::from(chunk[0]) << 16) | (u32::from(chunk[1]) << 8),
                1 => u32::from(chunk[0]) << 16,
                _ => 0,
            };

            let output_chars = chunk.len() + 1;

            for i in 0..4 {
                if i < output_chars {
                    let shift = 18 - i * 6;
                    let temp = buffer >> shift;
                    let index = (temp & 63) as usize;
                    encoded.push(BASE64_CHARSET[index]);
                } else {
                    encoded.push(b'=');
                }
            }
        }

        String::from_utf8(encoded).map_err(Base64Error::Utf8Error)
    }

    pub fn decode(&mut self, input: &str) -> Result<String, Base64Error> {
        let mut decoded = Vec::new();
        let mut buffer = 0u32;
        let mut bits_collected = 0;

        for c in input.chars() {
            if c != '=' {
                let position = BASE64_CHARSET.iter().position(|&x| x == c as u8);

                match position {
                    Some(pos) => {
                        buffer = (buffer << 6) | pos as u32;
                        bits_collected += 6;

                        while bits_collected >= 8 {
                            bits_collected -= 8;
                            let byte = (buffer >> bits_collected) & 0xFF;
                            decoded.push(byte as u8);
                        }
                    }
                    None => return Err(Base64Error::InvalidCharacter),
                }
            }
        }

        String::from_utf8(decoded).map_err(Base64Error::Utf8Error)
    }
}
