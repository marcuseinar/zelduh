//! Just enough of RFC 6455 to talk to a browser.
//!
//! A WebSocket handshake needs SHA-1 and base64 and nothing else, and the
//! frame format is a few bytes of header, so both are written out here rather
//! than pulling in a dependency that would have to be vendored, audited and
//! kept current for the sake of eighty lines of code.

use std::io::{self, Read, Write};

/// The fixed string RFC 6455 says to append to the client's key.
const GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

/// SHA-1, as required by the handshake.
pub fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [
        0x6745_2301,
        0xEFCD_AB89,
        0x98BA_DCFE,
        0x1032_5476,
        0xC3D2_E1F0,
    ];
    let mut msg = data.to_vec();
    let bit_len = (data.len() as u64) * 8;
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for block in msg.chunks(64) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                block[i * 4],
                block[i * 4 + 1],
                block[i * 4 + 2],
                block[i * 4 + 3],
            ]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let (mut a, mut b, mut c, mut d, mut e) = (h[0], h[1], h[2], h[3], h[4]);
        for (i, word) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A82_7999),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
    }

    let mut out = [0u8; 20];
    for (i, v) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&v.to_be_bytes());
    }
    out
}

/// Standard base64, with padding.
pub fn base64(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// The value for the `Sec-WebSocket-Accept` header.
pub fn accept_key(client_key: &str) -> String {
    let mut buf = client_key.trim().to_string();
    buf.push_str(GUID);
    base64(&sha1(buf.as_bytes()))
}

/// A WebSocket frame's opcode.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Opcode {
    Continuation,
    Text,
    Binary,
    Close,
    Ping,
    Pong,
    Other(u8),
}

impl Opcode {
    fn from_u8(v: u8) -> Opcode {
        match v {
            0 => Opcode::Continuation,
            1 => Opcode::Text,
            2 => Opcode::Binary,
            8 => Opcode::Close,
            9 => Opcode::Ping,
            10 => Opcode::Pong,
            other => Opcode::Other(other),
        }
    }

    fn to_u8(self) -> u8 {
        match self {
            Opcode::Continuation => 0,
            Opcode::Text => 1,
            Opcode::Binary => 2,
            Opcode::Close => 8,
            Opcode::Ping => 9,
            Opcode::Pong => 10,
            Opcode::Other(v) => v,
        }
    }
}

/// One frame read off the wire.
pub struct Frame {
    pub opcode: Opcode,
    pub payload: Vec<u8>,
}

/// The largest frame a client is allowed to send. Anything bigger is a bug or
/// an attack, not a game message.
const MAX_FRAME: u64 = 1 << 20;

/// Reads one frame, returning `None` at a clean end of stream.
pub fn read_frame(stream: &mut impl Read) -> io::Result<Option<Frame>> {
    let mut head = [0u8; 2];
    match stream.read_exact(&mut head) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let opcode = Opcode::from_u8(head[0] & 0x0f);
    let masked = head[1] & 0x80 != 0;
    let mut len = (head[1] & 0x7f) as u64;
    if len == 126 {
        let mut b = [0u8; 2];
        stream.read_exact(&mut b)?;
        len = u16::from_be_bytes(b) as u64;
    } else if len == 127 {
        let mut b = [0u8; 8];
        stream.read_exact(&mut b)?;
        len = u64::from_be_bytes(b);
    }
    if len > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frame is too large",
        ));
    }
    let mut mask = [0u8; 4];
    if masked {
        stream.read_exact(&mut mask)?;
    }
    let mut payload = vec![0u8; len as usize];
    stream.read_exact(&mut payload)?;
    if masked {
        for (i, b) in payload.iter_mut().enumerate() {
            *b ^= mask[i % 4];
        }
    }
    Ok(Some(Frame { opcode, payload }))
}

/// Writes one unmasked frame, as a server must.
pub fn write_frame(stream: &mut impl Write, opcode: Opcode, payload: &[u8]) -> io::Result<()> {
    let mut head = Vec::with_capacity(10);
    head.push(0x80 | opcode.to_u8());
    let len = payload.len();
    if len < 126 {
        head.push(len as u8);
    } else if len <= u16::MAX as usize {
        head.push(126);
        head.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        head.push(127);
        head.extend_from_slice(&(len as u64).to_be_bytes());
    }
    stream.write_all(&head)?;
    stream.write_all(payload)?;
    stream.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha1_matches_the_published_vectors() {
        assert_eq!(
            sha1(b"abc"),
            [
                0xa9, 0x99, 0x3e, 0x36, 0x47, 0x06, 0x81, 0x6a, 0xba, 0x3e, 0x25, 0x71, 0x78, 0x50,
                0xc2, 0x6c, 0x9c, 0xd0, 0xd8, 0x9d
            ]
        );
        assert_eq!(
            sha1(b""),
            [
                0xda, 0x39, 0xa3, 0xee, 0x5e, 0x6b, 0x4b, 0x0d, 0x32, 0x55, 0xbf, 0xef, 0x95, 0x60,
                0x18, 0x90, 0xaf, 0xd8, 0x07, 0x09
            ]
        );
    }

    #[test]
    fn sha1_handles_a_message_that_spans_blocks() {
        // 448 bits, the boundary case where padding needs a second block.
        let long = "abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
        assert_eq!(
            sha1(long.as_bytes()),
            [
                0x84, 0x98, 0x3e, 0x44, 0x1c, 0x3b, 0xd2, 0x6e, 0xba, 0xae, 0x4a, 0xa1, 0xf9, 0x51,
                0x29, 0xe5, 0xe5, 0x46, 0x70, 0xf1
            ]
        );
    }

    #[test]
    fn base64_matches_the_published_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn the_handshake_key_matches_the_rfc_example() {
        // Straight out of RFC 6455 section 1.3.
        assert_eq!(
            accept_key("dGhlIHNhbXBsZSBub25jZQ=="),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
    }

    #[test]
    fn frames_round_trip() {
        for len in [0usize, 5, 125, 126, 300, 70_000] {
            let payload = vec![0xa5u8; len];
            let mut buf = Vec::new();
            write_frame(&mut buf, Opcode::Binary, &payload).unwrap();
            let frame = read_frame(&mut buf.as_slice()).unwrap().unwrap();
            assert_eq!(frame.opcode, Opcode::Binary);
            assert_eq!(frame.payload, payload, "length {len}");
        }
    }

    #[test]
    fn masked_client_frames_are_unmasked() {
        // A client frame: FIN+binary, masked, 4 bytes.
        let mask = [0x01, 0x02, 0x03, 0x04];
        let data = [0x10, 0x20, 0x30, 0x40];
        let mut raw = vec![0x82, 0x84];
        raw.extend_from_slice(&mask);
        for (i, b) in data.iter().enumerate() {
            raw.push(b ^ mask[i % 4]);
        }
        let frame = read_frame(&mut raw.as_slice()).unwrap().unwrap();
        assert_eq!(frame.payload, data);
    }

    #[test]
    fn an_empty_stream_ends_cleanly() {
        let empty: &[u8] = &[];
        assert!(read_frame(&mut { empty }).unwrap().is_none());
    }

    #[test]
    fn an_absurd_length_is_refused() {
        // Claims a payload of 2^40 bytes.
        let raw = vec![0x82, 127, 0, 0, 1, 0, 0, 0, 0, 0];
        assert!(read_frame(&mut raw.as_slice()).is_err());
    }
}
