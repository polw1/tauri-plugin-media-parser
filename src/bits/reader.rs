/*
# Bits Reader Module

 Provides utilities for reading binary data from streams and byte arrays with bit-level precision.
 Includes both byte-aligned readers for common integer types (u8, u24, u32, u64) and a BitReader
 for arbitrary bit-width operations needed in video codec parsing and binary format processing.

 Key components:
 - Byte-aligned readers: `read_u8()`, `read_u24()`, `read_u32_be()`, `read_u64_be()`
 - Slice readers: `read_u32()`, `read_u64()` with position tracking
 - BitReader: Bit-precise reading with error accumulation for codec parsing
*/

use std::io::{self, Read};

/// Mask for the `n` least significant bits.
pub fn mask(n: u32) -> u32 {
    if n == 32 {
        u32::MAX
    } else {
        (1u32 << n) - 1
    }
}

/// Read one byte from a `Read` implementation.
pub fn read_u8<R: Read>(r: &mut R) -> io::Result<u8> {
    let mut buf = [0u8; 1];
    r.read_exact(&mut buf)?;
    Ok(buf[0])
}

/// Read a 24-bit big endian value from `r`.
pub fn read_u24<R: Read>(r: &mut R) -> io::Result<u32> {
    let mut buf = [0u8; 3];
    r.read_exact(&mut buf)?;
    Ok(((buf[0] as u32) << 16) | ((buf[1] as u32) << 8) | buf[2] as u32)
}

/// Read a 32-bit big endian value from `r`.
pub fn read_u32_be<R: Read>(r: &mut R) -> io::Result<u32> {
    let mut buf = [0u8; 4];
    r.read_exact(&mut buf)?;
    Ok(u32::from_be_bytes(buf))
}

/// Read a 64-bit big endian value from `r`.
pub fn read_u64_be<R: Read>(r: &mut R) -> io::Result<u64> {
    let mut buf = [0u8; 8];
    r.read_exact(&mut buf)?;
    Ok(u64::from_be_bytes(buf))
}

/// Read a 32-bit big endian value from a byte slice advancing the position.
pub fn read_u32(data: &[u8], pos: &mut usize) -> Option<u32> {
    if *pos + 4 > data.len() {
        return None;
    }
    let v = u32::from_be_bytes([data[*pos], data[*pos + 1], data[*pos + 2], data[*pos + 3]]);
    *pos += 4;
    Some(v)
}

/// Read a 64-bit big endian value from a byte slice advancing the position.
pub fn read_u64(data: &[u8], pos: &mut usize) -> Option<u64> {
    if *pos + 8 > data.len() {
        return None;
    }
    let v = u64::from_be_bytes([
        data[*pos],
        data[*pos + 1],
        data[*pos + 2],
        data[*pos + 3],
        data[*pos + 4],
        data[*pos + 5],
        data[*pos + 6],
        data[*pos + 7],
    ]);
    *pos += 8;
    Some(v)
}

/// `BitReader` reads bits from an underlying reader and accumulates the first
/// error that occurs.
#[derive(Debug)]
pub struct BitReader<R: Read> {
    rd: R,
    err: Option<io::Error>,
    n: u32,
    value: u64,
    pos: i64,
}

impl<R: Read> BitReader<R> {
    /// Create a new `BitReader` that starts accumulating errors.
    pub fn new(rd: R) -> Self {
        Self {
            rd,
            err: None,
            n: 0,
            value: 0,
            pos: -1,
        }
    }

    /// Return the accumulated error if any.
    pub fn acc_error(&self) -> Option<&io::Error> {
        self.err.as_ref()
    }

    /// Read `n` bits and return them as the lowest bits of a `u32`.
    /// If an error has occurred, 0 is returned.
    pub fn read(&mut self, n: u32) -> u32 {
        if self.err.is_some() {
            return 0;
        }
        while self.n < n {
            let mut buf = [0u8; 1];
            match self.rd.read_exact(&mut buf) {
                Ok(()) => {
                    self.pos += 1;
                    self.value = (self.value << 8) | u64::from(buf[0]);
                    self.n += 8;
                }
                Err(e) => {
                    self.err = Some(e);
                    return 0;
                }
            }
        }
        let value = (self.value >> (self.n - n)) as u32;
        self.n -= n;
        self.value &= (1u64 << self.n) - 1;
        value
    }

    /// Read `n` bits and interpret as a signed integer.
    pub fn read_signed(&mut self, n: u32) -> i32 {
        let v = self.read(n);
        if n == 0 {
            return 0;
        }
        let first = v >> (n - 1);
        if first == 1 {
            (v as i32) | (!0 << n)
        } else {
            v as i32
        }
    }

    /// Read a single bit interpreted as a boolean flag.
    pub fn read_flag(&mut self) -> bool {
        self.read(1) == 1
    }

    /// Read remaining bytes if currently byte-aligned.
    pub fn read_remaining_bytes(&mut self) -> Option<Vec<u8>> {
        if self.err.is_some() {
            return None;
        }
        if self.n != 0 {
            self.err = Some(io::Error::other(format!(
                "{} bit instead of byte alignment when reading remaining bytes",
                self.n
            )));
            return None;
        }
        let mut rest = Vec::new();
        if let Err(e) = self.rd.read_to_end(&mut rest) {
            self.err = Some(e);
            return None;
        }
        Some(rest)
    }

    /// Number of bytes read from the underlying reader.
    pub fn nr_bytes_read(&self) -> i64 {
        self.pos + 1
    }

    /// Total number of bits read.
    pub fn nr_bits_read(&self) -> i64 {
        let mut nr = self.nr_bytes_read() * 8;
        if self.nr_bits_read_in_current_byte() != 8 {
            nr += self.nr_bits_read_in_current_byte() - 8;
        }
        nr
    }

    /// Number of bits consumed in the current byte.
    pub fn nr_bits_read_in_current_byte(&self) -> i64 {
        8 - self.n as i64
    }
}

#[cfg(test)]
mod tests {
    use super::{mask, BitReader};
    use std::io::Cursor;

    #[test]
    fn test_read_bits() {
        let data = [0xffu8, 0x0f];
        let mut r = BitReader::new(Cursor::new(&data));
        assert_eq!(r.read(2), 3); // 11
        assert_eq!(r.read(3), 7); // 111
        assert_eq!(r.read(5), 28); // 11100
        assert_eq!(r.read(3), 1); // 001
        assert_eq!(r.read(3), 7); // 111
        assert!(r.acc_error().is_none());
    }

    #[test]
    fn test_read_signed_bits() {
        let data = [0xffu8, 0x0c];
        let mut r = BitReader::new(Cursor::new(&data));
        assert_eq!(r.read_signed(2), -1);
        assert_eq!(r.read_signed(3), -1);
        assert_eq!(r.read_signed(5), -4);
        assert_eq!(r.read_signed(3), 1);
        assert_eq!(r.read_signed(3), -4);
        assert!(r.acc_error().is_none());
    }

    #[test]
    fn test_writer_mask() {
        assert_eq!(mask(8), 0xff);
        assert_eq!(mask(4), 0x0f);
    }
}
