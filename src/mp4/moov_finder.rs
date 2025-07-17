use crate::streams::seekable_stream::SeekableStream;
use std::io::{self, SeekFrom};

/// Initial search window in bytes (8KB)
const INITIAL_SEARCH_SIZE: usize = 8192;
/// Additional bytes to scan from the start if the moov box is not found in the initial window
const FALLBACK_SEARCH_LIMIT: usize = 512 * 1024; // 512KB
/// Additional bytes to scan from the end of the file
const TRAILER_SEARCH_LIMIT: usize = 512 * 1024; // 512KB

/// Result of finding a moov box
#[derive(Debug, Clone)]
pub struct MoovBoxInfo {
    pub position: u64,
    pub size: u64,
}

/// Find the moov box efficiently by checking beginning and end of file
/// Returns position and size of the moov box
pub fn find_moov_box_efficiently<S: SeekableStream>(stream: &mut S) -> io::Result<MoovBoxInfo> {
    // Try to find moov box at the beginning (first 8KB)
    stream.seek(SeekFrom::Start(0))?;
    let mut buffer = vec![0u8; INITIAL_SEARCH_SIZE];
    let bytes_read = stream.read(&mut buffer)?;

    // Look for moov box in the first chunk
    for i in 0..bytes_read.saturating_sub(8) {
        if &buffer[i + 4..i + 8] == b"moov" {
            let size =
                u32::from_be_bytes([buffer[i], buffer[i + 1], buffer[i + 2], buffer[i + 3]]) as u64;
            return Ok(MoovBoxInfo {
                position: i as u64,
                size,
            });
        }
    }

    // If not found at beginning, try the end of the file (last 8KB)
    let file_size = stream.seek(SeekFrom::End(0))?;
    if file_size > INITIAL_SEARCH_SIZE as u64 {
        let search_start = file_size - INITIAL_SEARCH_SIZE as u64;
        stream.seek(SeekFrom::Start(search_start))?;
        buffer.clear();
        buffer.resize(INITIAL_SEARCH_SIZE, 0);
        let bytes_read = stream.read(&mut buffer)?;

        for i in 0..bytes_read.saturating_sub(8) {
            if &buffer[i + 4..i + 8] == b"moov" {
                let size =
                    u32::from_be_bytes([buffer[i], buffer[i + 1], buffer[i + 2], buffer[i + 3]])
                        as u64;
                return Ok(MoovBoxInfo {
                    position: search_start + i as u64,
                    size,
                });
            }
        }
    }

    // Fallback: scan a bit more from the start if moov is still not found
    let search_limit = std::cmp::min(file_size as usize, FALLBACK_SEARCH_LIMIT);
    let mut offset = INITIAL_SEARCH_SIZE as u64;
    while offset < search_limit as u64 {
        stream.seek(SeekFrom::Start(offset))?;
        buffer.clear();
        let remaining = search_limit as u64 - offset;
        let read_size = INITIAL_SEARCH_SIZE.min(remaining as usize);
        buffer.resize(read_size, 0);
        let bytes_read = stream.read(&mut buffer)?;
        for i in 0..bytes_read.saturating_sub(8) {
            if &buffer[i + 4..i + 8] == b"moov" {
                let size =
                    u32::from_be_bytes([buffer[i], buffer[i + 1], buffer[i + 2], buffer[i + 3]])
                        as u64;
                return Ok(MoovBoxInfo {
                    position: offset + i as u64,
                    size,
                });
            }
        }
        offset += bytes_read as u64;
        if bytes_read == 0 {
            break;
        }
    }

    // Fallback: scan from the end of the file
    let trailer_start = file_size.saturating_sub(TRAILER_SEARCH_LIMIT as u64);
    let mut offset = file_size.saturating_sub(INITIAL_SEARCH_SIZE as u64);
    loop {
        stream.seek(SeekFrom::Start(offset))?;
        buffer.clear();
        let remaining = file_size - offset;
        let read_size = INITIAL_SEARCH_SIZE.min(remaining as usize);
        buffer.resize(read_size, 0);
        let bytes_read = stream.read(&mut buffer)?;
        for i in 0..bytes_read.saturating_sub(8) {
            if &buffer[i + 4..i + 8] == b"moov" {
                let size =
                    u32::from_be_bytes([buffer[i], buffer[i + 1], buffer[i + 2], buffer[i + 3]])
                        as u64;
                return Ok(MoovBoxInfo {
                    position: offset + i as u64,
                    size,
                });
            }
        }
        if offset <= trailer_start {
            break;
        }
        offset = offset.saturating_sub(INITIAL_SEARCH_SIZE as u64);
    }

    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "moov box not found",
    ))
}

/// Find moov box and read its payload data
/// Returns the moov box payload (without the 8-byte header)
pub fn find_and_read_moov_box<S: SeekableStream>(stream: &mut S) -> io::Result<Vec<u8>> {
    let moov_info = find_moov_box_efficiently(stream)?;

    // Seek to the moov box and read it
    stream.seek(SeekFrom::Start(moov_info.position))?;
    let mut moov_data = vec![0u8; moov_info.size as usize];
    stream.read_exact(&mut moov_data)?;

    // Return payload without the 8-byte header (size + type)
    Ok(moov_data[8..].to_vec())
}
