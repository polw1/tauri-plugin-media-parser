//! Byte reading functions for big-endian, little-endian and native-endian data.

/// Reads a big-endian `u16` from `buf` at `offset`.
///
/// Returns `None` if `offset + 2 > buf.len()`.
#[inline]
pub fn read_u16_be(buf: &[u8], offset: usize) -> Option<u16> {
   let end = offset.checked_add(2)?;
   let bytes: [u8; 2] = buf.get(offset..end)?.try_into().ok()?;
   Some(u16::from_be_bytes(bytes))
}

/// Reads a little-endian `u16` from `buf` at `offset`.
///
/// Returns `None` if `offset + 2 > buf.len()`.
#[inline]
pub fn read_u16_le(buf: &[u8], offset: usize) -> Option<u16> {
   let end = offset.checked_add(2)?;
   let bytes: [u8; 2] = buf.get(offset..end)?.try_into().ok()?;
   Some(u16::from_le_bytes(bytes))
}

/// Reads a big-endian `u32` from `buf` at `offset`.
///
/// Returns `None` if `offset + 4 > buf.len()`.
#[inline]
pub fn read_u32_be(buf: &[u8], offset: usize) -> Option<u32> {
   let end = offset.checked_add(4)?;
   let bytes: [u8; 4] = buf.get(offset..end)?.try_into().ok()?;
   Some(u32::from_be_bytes(bytes))
}

/// Reads a big-endian `u64` from `buf` at `offset`.
///
/// Returns `None` if `offset + 8 > buf.len()`.
#[inline]
pub fn read_u64_be(buf: &[u8], offset: usize) -> Option<u64> {
   let end = offset.checked_add(8)?;
   let bytes: [u8; 8] = buf.get(offset..end)?.try_into().ok()?;
   Some(u64::from_be_bytes(bytes))
}

/// Reads a native-endian `u32` from `buf` at `offset`.
///
/// Native byte order is only correct for data this machine produced itself,
/// such as a C struct read out of its own memory.
///
/// Returns `None` if `offset + 4 > buf.len()`.
#[inline]
#[cfg(any(test, all(target_os = "android", feature = "android-mediacodec")))]
pub fn read_u32_ne(buf: &[u8], offset: usize) -> Option<u32> {
   let end = offset.checked_add(4)?;
   let bytes: [u8; 4] = buf.get(offset..end)?.try_into().ok()?;
   Some(u32::from_ne_bytes(bytes))
}

/// Reads a native-endian `i32` from `buf` at `offset`.
///
/// Native byte order is only correct for data this machine produced itself,
/// such as a C struct read out of its own memory.
///
/// Returns `None` if `offset + 4 > buf.len()`.
#[inline]
#[cfg(any(test, all(target_os = "android", feature = "android-mediacodec")))]
pub fn read_i32_ne(buf: &[u8], offset: usize) -> Option<i32> {
   let end = offset.checked_add(4)?;
   let bytes: [u8; 4] = buf.get(offset..end)?.try_into().ok()?;
   Some(i32::from_ne_bytes(bytes))
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn test_read_u16_be() {
      let buf = [0x12, 0x34];
      assert_eq!(read_u16_be(&buf, 0), Some(0x1234));
   }

   #[test]
   fn test_read_u16_le() {
      let buf = [0x34, 0x12];
      assert_eq!(read_u16_le(&buf, 0), Some(0x1234));
   }

   #[test]
   fn test_read_u32_be() {
      let buf = [0x12, 0x34, 0x56, 0x78];
      assert_eq!(read_u32_be(&buf, 0), Some(0x12345678));
   }

   #[test]
   fn test_read_u64_be() {
      let buf = [0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0];
      assert_eq!(read_u64_be(&buf, 0), Some(0x123456789ABCDEF0));
   }

   #[test]
   fn test_read_u32_ne() {
      let buf = 0x12345678u32.to_ne_bytes();
      assert_eq!(read_u32_ne(&buf, 0), Some(0x12345678));
   }

   #[test]
   fn test_read_i32_ne() {
      let buf = (-2i32).to_ne_bytes();
      assert_eq!(read_i32_ne(&buf, 0), Some(-2));
   }

   #[test]
   fn test_read_with_offset() {
      let buf = [0x00, 0x00, 0x12, 0x34, 0x56, 0x78];
      assert_eq!(read_u32_be(&buf, 2), Some(0x12345678));
   }

   #[test]
   fn test_read_out_of_bounds() {
      let buf = [0x12, 0x34];
      assert_eq!(read_u16_be(&buf, 1), None);
      assert_eq!(read_u32_be(&buf, 0), None);
      assert_eq!(read_u64_be(&buf, 0), None);
      assert_eq!(read_u32_ne(&buf, 0), None);
      assert_eq!(read_i32_ne(&buf, 0), None);
   }

   #[test]
   fn test_read_with_overflowing_offset_returns_none() {
      let buf = [0u8; 8];

      assert_eq!(read_u16_be(&buf, usize::MAX), None);
      assert_eq!(read_u16_le(&buf, usize::MAX), None);
      assert_eq!(read_u32_be(&buf, usize::MAX), None);
      assert_eq!(read_u64_be(&buf, usize::MAX), None);
      assert_eq!(read_u32_ne(&buf, usize::MAX), None);
      assert_eq!(read_i32_ne(&buf, usize::MAX), None);
   }

   #[test]
   fn test_read_empty_buffer() {
      let buf: [u8; 0] = [];
      assert_eq!(read_u16_be(&buf, 0), None);
      assert_eq!(read_u16_le(&buf, 0), None);
      assert_eq!(read_u32_be(&buf, 0), None);
      assert_eq!(read_u64_be(&buf, 0), None);
      assert_eq!(read_u32_ne(&buf, 0), None);
      assert_eq!(read_i32_ne(&buf, 0), None);
   }
}
