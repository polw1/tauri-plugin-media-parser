use crate::stream_reader::StreamReader;
use std::io::{self};

const INITIAL_SEARCH_SIZE: usize = 8192;
const FALLBACK_SEARCH_LIMIT: usize = 512 * 1024;
const TRAILER_SEARCH_LIMIT: usize = 512 * 1024;

#[derive(Debug, Clone)]
pub struct MoovBoxInfo {
   pub position: u64,
   pub size: u64,
}

pub async fn find_moov_box(stream: &mut dyn StreamReader) -> io::Result<MoovBoxInfo> {
   let file_size = stream
      .size()
      .await?
      .ok_or_else(|| io::Error::other("unknown size not supported for moov scan"))?;

   let head_len = INITIAL_SEARCH_SIZE.min(file_size as usize);
   let tail_len = INITIAL_SEARCH_SIZE.min(file_size as usize);

   // Read head and tail in parallel without changing stream cursor
   let ranges = if file_size > tail_len as u64 {
      vec![(0u64, head_len), (file_size - tail_len as u64, tail_len)]
   } else {
      vec![(0u64, head_len)]
   };
   let bufs = stream.read_ranges(&ranges).await?;
   // Scan head
   if let Some(buf0) = bufs.get(0) {
      if let Some(info) = scan_buffer(buf0, 0, file_size, false) {
         return Ok(info);
      }
   }
   // Scan tail (adjusting position)
   if bufs.len() > 1 {
      if let Some(info) = scan_buffer(&bufs[1], file_size - tail_len as u64, file_size, false) {
         return Ok(info);
      }
   }

   // Fallback: scan first FALLBACK_SEARCH_LIMIT bytes from start (no overlap)
   let search_limit = std::cmp::min(file_size as usize, FALLBACK_SEARCH_LIMIT);
   let mut offset = head_len as u64;
   while (offset as usize) < search_limit {
      let remaining = (search_limit as u64).saturating_sub(offset);
      let read_size = INITIAL_SEARCH_SIZE.min(remaining as usize);
      let buf = stream.read_at(offset, read_size).await?;
      if let Some(info) = scan_buffer(&buf, offset, file_size, false) {
         return Ok(info);
      }
      if buf.is_empty() { break; }
      offset = offset.saturating_add(buf.len() as u64);
   }

   // Trailer fallback: scan last TRAILER_SEARCH_LIMIT bytes backward
   let trailer_start = file_size.saturating_sub(TRAILER_SEARCH_LIMIT as u64);
   let mut offset = file_size.saturating_sub(tail_len as u64);
   loop {
      let remaining = file_size.saturating_sub(offset);
      let read_size = INITIAL_SEARCH_SIZE.min(remaining as usize);
      let buf = stream.read_at(offset, read_size).await?;
      if let Some(info) = scan_buffer(&buf, offset, file_size, false) {
         return Ok(info);
      }
      if offset <= trailer_start { break; }
      if offset < INITIAL_SEARCH_SIZE as u64 { break; }
      offset = offset.saturating_sub(INITIAL_SEARCH_SIZE as u64);
   }

   Err(io::Error::new(
      io::ErrorKind::NotFound,
      "moov box not found",
   ))
}

fn scan_buffer(buf: &[u8], base_offset: u64, file_size: u64, confirm_mvhd: bool) -> Option<MoovBoxInfo> {
   let mut off = 0usize;
   while off + 8 <= buf.len() {
      let size32 = u32::from_be_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]]) as u64;
      let typ = &buf[off + 4..off + 8];
      let mut box_size = size32;
      let header_len = if size32 == 1 {
         if off + 16 > buf.len() {
            // incomplete largesize header in this window; defer to overlap/next read
            break;
         }
         box_size = u64::from_be_bytes([
            buf[off + 8], buf[off + 9], buf[off + 10], buf[off + 11],
            buf[off + 12], buf[off + 13], buf[off + 14], buf[off + 15],
         ]);
         16usize
      } else {
         8usize
      };
      if box_size < header_len as u64 {
         // invalid size; stop scanning this buffer
         break;
      }

      if typ == b"moov" {
         // Validate size within file bounds
         let abs_pos = base_offset.saturating_add(off as u64);
         if abs_pos.saturating_add(box_size) <= file_size {
            // Optional: confirm presence of mvhd within available payload
            if confirm_mvhd {
               let payload_start = off + header_len;
               let payload_end = (off as u64 + box_size).min(buf.len() as u64) as usize;
               if payload_end > payload_start {
                  let payload = &buf[payload_start..payload_end];
                  if !payload.windows(4).any(|w| w == b"mvhd") {
                     // couldn't confirm; still accept, or continue? choose accept for small windows
                  }
               }
            }
            return Some(MoovBoxInfo { position: abs_pos, size: box_size });
         }
      }

      // advance to next box
      // if box_size goes beyond buffer, stop to avoid infinite loop
      if box_size == 0 { break; }
      if off as u64 + box_size <= buf.len() as u64 {
         off += box_size as usize;
      } else {
         break;
      }
   }
   None
}

pub async fn find_and_read_moov_box(stream: &mut dyn StreamReader) -> io::Result<Vec<u8>> {
   let info = find_moov_box(stream).await?;
   stream.read_at(info.position, info.size as usize).await
}
