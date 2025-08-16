use bytes::{Buf, BufMut, BytesMut};
use std::io;

const KISS_FEND: u8 = 0xC0;
const KISS_FESC: u8 = 0xDB;
const KISS_TFEND: u8 = 0xDC;
const KISS_TFESC: u8 = 0xDD;

const KISS_CMD_DATA: u8 = 0x00;
#[cfg(test)]
const KISS_CMD_TXDELAY: u8 = 0x01;

pub struct KissCodec {
    decode_buf: BytesMut,
    in_frame: bool,
    escaped: bool,
}

impl KissCodec {
    pub fn new() -> Self {
        KissCodec {
            decode_buf: BytesMut::with_capacity(1024),
            in_frame: false,
            escaped: false,
        }
    }

    pub fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Vec<u8>>, io::Error> {
        while src.has_remaining() {
            let byte = src.get_u8();

            if self.escaped {
                self.escaped = false;
                match byte {
                    KISS_TFEND => self.decode_buf.put_u8(KISS_FEND),
                    KISS_TFESC => self.decode_buf.put_u8(KISS_FESC),
                    _ => {
                        self.decode_buf.clear();
                        self.in_frame = false;
                    }
                }
                continue;
            }

            match byte {
                KISS_FEND => {
                    if self.in_frame && !self.decode_buf.is_empty() {
                        let frame = self.decode_buf.split().to_vec();
                        self.in_frame = false;

                        if !frame.is_empty() {
                            let cmd = frame[0] & 0x0F;
                            let port = (frame[0] >> 4) & 0x0F;
                            if cmd == KISS_CMD_DATA && port == 0 && frame.len() > 1 {
                                return Ok(Some(frame[1..].to_vec()));
                            }
                        }
                    } else {
                        self.in_frame = true;
                        self.decode_buf.clear();
                    }
                }
                KISS_FESC => {
                    if self.in_frame {
                        self.escaped = true;
                    }
                }
                _ => {
                    if self.in_frame {
                        self.decode_buf.put_u8(byte);
                    }
                }
            }
        }

        Ok(None)
    }

    pub fn encode(&self, data: &[u8], port: u8) -> Vec<u8> {
        let mut output = Vec::with_capacity(data.len() + 4);

        output.push(KISS_FEND);
        output.push((port << 4) | KISS_CMD_DATA);

        for &byte in data {
            match byte {
                KISS_FEND => {
                    output.push(KISS_FESC);
                    output.push(KISS_TFEND);
                }
                KISS_FESC => {
                    output.push(KISS_FESC);
                    output.push(KISS_TFESC);
                }
                _ => output.push(byte),
            }
        }

        output.push(KISS_FEND);
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kiss_encode() {
        let codec = KissCodec::new();

        // Simple data
        let data = b"Hello";
        let encoded = codec.encode(data, 0);
        assert_eq!(encoded[0], KISS_FEND);
        assert_eq!(encoded[1], KISS_CMD_DATA);
        assert_eq!(&encoded[2..7], b"Hello");
        assert_eq!(encoded[7], KISS_FEND);

        // Data with FEND
        let data = &[0x41, KISS_FEND, 0x42];
        let encoded = codec.encode(data, 0);
        assert_eq!(encoded[0], KISS_FEND);
        assert_eq!(encoded[1], KISS_CMD_DATA);
        assert_eq!(encoded[2], 0x41);
        assert_eq!(encoded[3], KISS_FESC);
        assert_eq!(encoded[4], KISS_TFEND);
        assert_eq!(encoded[5], 0x42);
        assert_eq!(encoded[6], KISS_FEND);

        // Data with FESC
        let data = &[0x41, KISS_FESC, 0x42];
        let encoded = codec.encode(data, 0);
        assert_eq!(encoded[3], KISS_FESC);
        assert_eq!(encoded[4], KISS_TFESC);

        // Different port
        let encoded = codec.encode(b"Test", 1);
        assert_eq!(encoded[1], 0x10); // Port 1, command 0
    }

    #[test]
    fn test_kiss_decode_simple() {
        let mut codec = KissCodec::new();
        let mut buf = BytesMut::new();

        // Simple frame
        buf.extend_from_slice(&[KISS_FEND, KISS_CMD_DATA, 0x41, 0x42, KISS_FEND]);

        let result = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(result, vec![0x41, 0x42]);
    }

    #[test]
    fn test_kiss_decode_escaped() {
        let mut codec = KissCodec::new();
        let mut buf = BytesMut::new();

        // Frame with escaped FEND
        buf.extend_from_slice(&[
            KISS_FEND,
            KISS_CMD_DATA,
            0x41,
            KISS_FESC,
            KISS_TFEND,
            0x42,
            KISS_FEND,
        ]);

        let result = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(result, vec![0x41, KISS_FEND, 0x42]);

        // Frame with escaped FESC
        buf.extend_from_slice(&[
            KISS_FEND,
            KISS_CMD_DATA,
            0x41,
            KISS_FESC,
            KISS_TFESC,
            0x42,
            KISS_FEND,
        ]);

        let result = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(result, vec![0x41, KISS_FESC, 0x42]);
    }

    #[test]
    fn test_kiss_decode_multiple_frames() {
        let mut codec = KissCodec::new();
        let mut buf = BytesMut::new();

        // Two frames back-to-back
        buf.extend_from_slice(&[
            KISS_FEND,
            KISS_CMD_DATA,
            0x41,
            KISS_FEND,
            KISS_FEND,
            KISS_CMD_DATA,
            0x42,
            KISS_FEND,
        ]);

        let result1 = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(result1, vec![0x41]);

        let result2 = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(result2, vec![0x42]);
    }

    #[test]
    fn test_kiss_decode_partial() {
        let mut codec = KissCodec::new();
        let mut buf = BytesMut::new();

        // Partial frame
        buf.extend_from_slice(&[KISS_FEND, KISS_CMD_DATA, 0x41]);
        assert!(codec.decode(&mut buf).unwrap().is_none());

        // Complete the frame
        buf.extend_from_slice(&[0x42, KISS_FEND]);
        let result = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(result, vec![0x41, 0x42]);
    }

    #[test]
    fn test_kiss_decode_non_data_frames() {
        let mut codec = KissCodec::new();
        let mut buf = BytesMut::new();

        // TXDELAY frame (should be ignored)
        buf.extend_from_slice(&[KISS_FEND, KISS_CMD_TXDELAY, 0x10, KISS_FEND]);
        assert!(codec.decode(&mut buf).unwrap().is_none());

        // Different port data frame
        buf.extend_from_slice(&[KISS_FEND, 0x10, 0x41, 0x42, KISS_FEND]);
        assert!(codec.decode(&mut buf).unwrap().is_none());
    }

    #[test]
    fn test_kiss_decode_invalid_escape() {
        let mut codec = KissCodec::new();
        let mut buf = BytesMut::new();

        // Invalid escape sequence
        buf.extend_from_slice(&[KISS_FEND, KISS_CMD_DATA, KISS_FESC, 0xFF, KISS_FEND]);
        assert!(codec.decode(&mut buf).unwrap().is_none());

        // Codec should recover for next frame
        buf.extend_from_slice(&[KISS_FEND, KISS_CMD_DATA, 0x41, KISS_FEND]);
        let result = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(result, vec![0x41]);
    }
}
