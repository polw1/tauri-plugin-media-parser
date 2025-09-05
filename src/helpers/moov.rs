use crate::stream_reader::StreamReader;
use std::io::{self, SeekFrom};

const INITIAL_SEARCH_SIZE: usize = 8192;
const FALLBACK_SEARCH_LIMIT: usize = 512 * 1024;
const TRAILER_SEARCH_LIMIT: usize = 512 * 1024;

#[derive(Debug, Clone)]
pub struct MoovBoxInfo {
   pub position: u64,
   pub size: u64,
}

pub async fn find_moov_box(stream: &mut dyn StreamReader) -> io::Result<MoovBoxInfo> {
   stream.seek(SeekFrom::Start(0)).await?;
   let mut buffer = vec![0u8; INITIAL_SEARCH_SIZE];
   let bytes_read = stream.read(&mut buffer).await?;
   if let Some(info) = scan_buffer(&buffer[..bytes_read]) {
      return Ok(info);
   }

   let file_size = stream.seek(SeekFrom::End(0)).await?;

   if file_size > INITIAL_SEARCH_SIZE as u64 {
      let start = file_size - INITIAL_SEARCH_SIZE as u64;
      stream.seek(SeekFrom::Start(start)).await?;
      buffer.fill(0);
      let bytes_read = stream.read(&mut buffer).await?;
      if let Some(mut info) = scan_buffer(&buffer[..bytes_read]) {
         info.position += start;
         return Ok(info);
      }
   }

   let search_limit = std::cmp::min(file_size as usize, FALLBACK_SEARCH_LIMIT);
   let mut offset = INITIAL_SEARCH_SIZE as u64;
   while offset < search_limit as u64 {
      stream.seek(SeekFrom::Start(offset)).await?;
      let remaining = search_limit as u64 - offset;
      let read_size = INITIAL_SEARCH_SIZE.min(remaining as usize);
      buffer.resize(read_size, 0);
      let bytes_read = stream.read(&mut buffer).await?;
      if let Some(mut info) = scan_buffer(&buffer[..bytes_read]) {
         info.position += offset;
         return Ok(info);
      }
      if bytes_read == 0 {
         break;
      }
      offset += bytes_read as u64;
   }

   let trailer_start = file_size.saturating_sub(TRAILER_SEARCH_LIMIT as u64);
   let mut offset = file_size.saturating_sub(INITIAL_SEARCH_SIZE as u64);
   loop {
      stream.seek(SeekFrom::Start(offset)).await?;
      let remaining = file_size - offset;
      let read_size = INITIAL_SEARCH_SIZE.min(remaining as usize);
      buffer.resize(read_size, 0);
      let bytes_read = stream.read(&mut buffer).await?;
      if let Some(mut info) = scan_buffer(&buffer[..bytes_read]) {
         info.position += offset;
         return Ok(info);
      }
      if offset <= trailer_start {
         break;
      }
      if offset < INITIAL_SEARCH_SIZE as u64 {
         break;
      }
      offset = offset.saturating_sub(INITIAL_SEARCH_SIZE as u64);
   }

   Err(io::Error::new(
      io::ErrorKind::NotFound,
      "moov box not found",
   ))
}

fn scan_buffer(buf: &[u8]) -> Option<MoovBoxInfo> {
   for i in 0..buf.len().saturating_sub(8) {
      if &buf[i + 4..i + 8] == b"moov" {
         let size = u32::from_be_bytes([buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]) as u64;
         return Some(MoovBoxInfo {
            position: i as u64,
            size,
         });
      }
   }
   None
}

pub async fn find_and_read_moov_box(stream: &mut dyn StreamReader) -> io::Result<Vec<u8>> {
   let info = find_moov_box(stream).await?;
   stream.seek(SeekFrom::Start(info.position)).await?;
   let mut data = vec![0u8; info.size as usize];
   let mut read = 0;
   while read < data.len() {
      let n = stream.read(&mut data[read..]).await?;
      if n == 0 {
         break;
      }
      read += n;
   }
   data.truncate(read);
   Ok(data)
}
