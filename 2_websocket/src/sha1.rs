use std::fmt;

// SHA-1 hashing algorithm initial hash values.
// These constants are derived from the fractional parts of the square roots of the first five primes.
const H0: u32 = 0x67452301;
const H1: u32 = 0xEFCDAB89;
const H2: u32 = 0x98BADCFE;
const H3: u32 = 0x10325476;
const H4: u32 = 0xC3D2E1F0;

#[derive(Debug)]
pub enum Sha1Error {
    InputConversionFailure(String),
}

impl fmt::Display for Sha1Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Sha1Error::InputConversionFailure(ref msg) => {
                write!(f, "Input conversion failed: {}", msg)
            }
        }
    }
}

impl std::error::Error for Sha1Error {}

pub struct Sha1;

impl Sha1 {
    /// Constructs a new `Sha1` hasher.
    pub fn new() -> Self {
        Sha1 {}
    }

    /// Computes the SHA-1 hash of the input string by taking in either a String of str type.
    pub fn hash(&mut self, key: String) -> Result<[u8; 20], Sha1Error> {
        // Initialize variables to the SHA-1's initial hash values.
        let (mut h0, mut h1, mut h2, mut h3, mut h4) = (H0, H1, H2, H3, H4);
        let (mut a, mut b, mut c, mut d, mut e);

        // Pad our key
        let msg = self.pad_message(key.as_ref());

        // Process each 512-bit chunk of the padded message.
        for chunk in msg.chunks(64) {
            // Get the message schedule and copies of our initial SHA-1 values.
            let schedule = self.build_schedule(chunk)?;

            a = h0;
            b = h1;
            c = h2;
            d = h3;
            e = h4;

            // Main loop of the SHA-1 algorithm using predefind values based on primes numbers.
            for i in 0..80 {
                let (f, k) = match i {
                    0..=19 => ((b & c) | ((!b) & d), 0x5A827999),
                    20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                    40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                    _ => (b ^ c ^ d, 0xCA62C1D6),
                };

                // Update the temporary variable and then update the hash values
                // in a manner that enforces both diffusion and confusion. Note
                // how the "scrambled" data trickles through the variables as we
                // loop through.
                let temp = a
                    .rotate_left(5)
                    .wrapping_add(f)
                    .wrapping_add(e)
                    .wrapping_add(k)
                    .wrapping_add(schedule[i]);
                e = d;
                d = c;
                c = b.rotate_left(30);
                b = a;
                a = temp;
            }

            // Add the compressed chunk to the current hash value.
            h0 = h0.wrapping_add(a);
            h1 = h1.wrapping_add(b);
            h2 = h2.wrapping_add(c);
            h3 = h3.wrapping_add(d);
            h4 = h4.wrapping_add(e);
        }

        // Produce the final hash value as a 20-byte array.
        let mut hash = [0u8; 20];

        hash[0..4].copy_from_slice(&h0.to_be_bytes());
        hash[4..8].copy_from_slice(&h1.to_be_bytes());
        hash[8..12].copy_from_slice(&h2.to_be_bytes());
        hash[12..16].copy_from_slice(&h3.to_be_bytes());
        hash[16..20].copy_from_slice(&h4.to_be_bytes());

        Ok(hash)
    }

    /// Pads the input message according to SHA-1 specifications.
    /// This includes appending a '1' bit followed by '0' bits and finally the message length.
    fn pad_message(&self, input: &str) -> Vec<u8> {
        let mut bytes = input.as_bytes().to_vec();

        // Save the original message length for appending below.
        let original_bit_length = bytes.len() as u64 * 8;

        // Append the '1' at the most most significant bit: 10000000
        bytes.push(0x80);

        // Pad with '0' bytes until the message's length in bits modules 512 is 448.
        while (bytes.len() * 8) % 512 != 448 {
            bytes.push(0);
        }

        // Append the original message length.
        bytes.extend_from_slice(&original_bit_length.to_be_bytes());

        bytes
    }

    /// Builds the message schedule array from a 512-bit chunk.
    fn build_schedule(&mut self, chunk: &[u8]) -> Result<[u32; 80], Sha1Error> {
        let mut schedule = [0u32; 80];

        // Initialize the first 16 words in the array from the chunk.
        for (i, block) in chunk.chunks(4).enumerate() {
            // Attempt to convert the block into u32
            let converted_block = block.try_into().map_err(|_| {
                Sha1Error::InputConversionFailure("Failed to convert chunk into u32".to_string())
            })?;
            schedule[i] = u32::from_be_bytes(converted_block);
        }

        // Extend the schedule array using previously defined values and the XOR (^) operation.
        for i in 16..80 {
            schedule[i] = schedule[i - 3] ^ schedule[i - 8] ^ schedule[i - 14] ^ schedule[i - 16];
            schedule[i] = schedule[i].rotate_left(1);
        }

        Ok(schedule)
    }
}
