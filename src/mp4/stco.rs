use super::r#box::find_box;
use crate::errors::{MediaParserError, MediaParserResult, Mp4Error};

/// Parse stco (chunk offset) or co64 box - unified function
pub fn parse_stco_or_co64(stbl: &[u8]) -> MediaParserResult<Vec<u64>> {
    // Try stco first (32-bit offsets)
    if let Some(stco) = find_box(stbl, "stco") {
        if stco.len() < 8 {
            return Err(MediaParserError::Mp4(Mp4Error::Error {
                message: "stco box too small: expected at least 8 bytes".to_string(),
            }));
        }
        let entry_count = u32::from_be_bytes([stco[4], stco[5], stco[6], stco[7]]);
        let mut offsets = Vec::new();

        for i in 0..entry_count {
            let offset_pos = 8 + (i * 4) as usize;
            if offset_pos + 4 <= stco.len() {
                let offset = u32::from_be_bytes([
                    stco[offset_pos],
                    stco[offset_pos + 1],
                    stco[offset_pos + 2],
                    stco[offset_pos + 3],
                ]) as u64;
                offsets.push(offset);
            }
        }
        return Ok(offsets);
    }

    // Try co64 (64-bit offsets)
    if let Some(co64) = find_box(stbl, "co64") {
        if co64.len() < 8 {
            return Err(MediaParserError::Mp4(Mp4Error::Error {
                message: "co64 box too small: expected at least 8 bytes".to_string(),
            }));
        }
        let entry_count = u32::from_be_bytes([co64[4], co64[5], co64[6], co64[7]]);
        let mut offsets = Vec::new();

        for i in 0..entry_count {
            let offset_pos = 8 + (i * 8) as usize;
            if offset_pos + 8 <= co64.len() {
                let offset = u64::from_be_bytes([
                    co64[offset_pos],
                    co64[offset_pos + 1],
                    co64[offset_pos + 2],
                    co64[offset_pos + 3],
                    co64[offset_pos + 4],
                    co64[offset_pos + 5],
                    co64[offset_pos + 6],
                    co64[offset_pos + 7],
                ]);
                offsets.push(offset);
            }
        }
        return Ok(offsets);
    }

    Err(MediaParserError::Mp4(Mp4Error::Error {
        message: "No chunk offset box found: missing both stco and co64".to_string(),
    }))
}

// Parse stco/co64 for thumbnails (strict error handling)
alias_strict!(parse_stco_or_co64_thumbnails, parse_stco_or_co64, Vec<u64>);
alias_lenient!(parse_stco_or_co64_subtitles, parse_stco_or_co64, Vec<u64>);
alias_lenient!(parse_stco_or_co64_lenient, parse_stco_or_co64, Vec<u64>);
