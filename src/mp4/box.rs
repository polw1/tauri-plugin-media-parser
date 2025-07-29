use crate::errors::{MediaParserError, MediaParserResult, Mp4Error};
use std::io::{Cursor, Read, Seek, SeekFrom};

use crate::bits::reader::{read_u32, read_u32_be, read_u64, read_u64_be};

/// Box header information
#[derive(Debug)]
pub struct BoxHeader {
    pub name: String,
    pub name_bytes: [u8; 4],
    pub size: u64,
    pub header_size: u64,
}

/// Read a box header from an io source
pub fn read_box_header<R: Read>(r: &mut R) -> MediaParserResult<BoxHeader> {
    let size32 = read_u32_be(r).map_err(|e| {
        MediaParserError::Mp4(Mp4Error::Error {
            message: format!("Failed to read box size: {}", e),
        })
    })?;
    let mut name_buf = [0u8; 4];
    r.read_exact(&mut name_buf).map_err(|e| {
        MediaParserError::Mp4(Mp4Error::Error {
            message: format!("Failed to read box name: {}", e),
        })
    })?;
    let mut size = size32 as u64;
    let mut header_size = 8u64;
    if size32 == 1 {
        size = read_u64_be(r).map_err(|e| {
            MediaParserError::Mp4(Mp4Error::Error {
                message: format!("Failed to read extended box size: {}", e),
            })
        })?;
        header_size = 16;
    }
    Ok(BoxHeader {
        name: String::from_utf8_lossy(&name_buf).into_owned(),
        name_bytes: name_buf,
        size,
        header_size,
    })
}

/// Parse a box header from a byte slice advancing the cursor
pub fn parse_box_header(data: &[u8], pos: &mut usize) -> Option<(String, u64)> {
    if *pos + 8 > data.len() {
        return None;
    }
    let size = read_u32(data, pos)? as u64;
    let name = &data[*pos..*pos + 4];
    *pos += 4;
    let mut real_size = size;
    if size == 1 {
        if *pos + 8 > data.len() {
            return None;
        }
        real_size = read_u64(data, pos)?;
    }
    Some((std::str::from_utf8(name).ok()?.to_string(), real_size))
}

/// Write a box header to a vector
pub fn write_box_header(output: &mut Vec<u8>, name: &str, size: u32) {
    output.extend_from_slice(&size.to_be_bytes());
    output.extend_from_slice(name.as_bytes());
}

/// Find a box and return the contained slice
pub fn find_box<'a>(data: &'a [u8], name: &str) -> Option<&'a [u8]> {
    let (_, start, end) = find_box_range(data, name)?;
    Some(&data[start..end])
}

/// Find a box and return the start and end indices of its payload
pub fn find_box_range(data: &[u8], name: &str) -> Option<(usize, usize, usize)> {
    let mut pos = 0usize;
    let mut iterations = 0; // Add safety counter

    while pos + 8 <= data.len() && iterations < 10000 {
        // Add iteration limit
        let start = pos;
        let (box_name, size) = parse_box_header(data, &mut pos)?;

        // Additional safety checks
        if size == 0 {
            // Skip empty boxes
            iterations += 1;
            continue;
        }

        if size < 8 {
            // Invalid box size
            return None;
        }

        if size as usize > data.len() - start {
            return None;
        }

        let payload_start = pos;
        let payload_end = start + size as usize;

        if box_name == name {
            return Some((start, payload_start, payload_end));
        }

        pos = payload_end;
        iterations += 1;

        // Additional safety: ensure we're making progress
        if pos <= start {
            return None;
        }
    }
    None
}

/// Parse name box function.
pub fn parse_name_box(data: &[u8], dest: &mut Option<String>) -> MediaParserResult<()> {
    let mut cursor = Cursor::new(data);
    let len = data.len() as u64;
    let mut pos = 0u64;
    while pos < len {
        let header = read_box_header(&mut cursor)?;
        let payload = header.size.saturating_sub(header.header_size);
        if &header.name_bytes == b"data" {
            cursor.seek(SeekFrom::Current(8)).map_err(|e| {
                MediaParserError::Mp4(Mp4Error::Error {
                    message: format!("Failed to seek past type and locale in data box: {}", e),
                })
            })?;
            let mut buf = vec![0u8; (payload - 8) as usize];
            cursor.read_exact(&mut buf).map_err(|e| {
                MediaParserError::Mp4(Mp4Error::Error {
                    message: format!("Failed to read data box content: {}", e),
                })
            })?;
            if let Ok(s) = String::from_utf8(buf) {
                *dest = Some(s);
            }
            break;
        } else {
            cursor
                .seek(SeekFrom::Current(payload as i64))
                .map_err(|e| {
                    MediaParserError::Mp4(Mp4Error::Error {
                        message: format!("Failed to skip box {}: {}", header.name, e),
                    })
                })?;
        }
        pos += header.size;
    }
    Ok(())
}
