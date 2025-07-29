use super::r#box::find_box;
use crate::errors::{MediaParserError, MediaParserResult, Mp4Error};

#[derive(Debug, PartialEq)]
pub struct SttsEntry {
    pub sample_count: u32,
    pub sample_delta: u32,
}

/// Parse stts (sample timing) box - unified function
pub fn parse_stts(stbl: &[u8]) -> MediaParserResult<Vec<SttsEntry>> {
    let stts = find_box(stbl, "stts").ok_or_else(|| {
        MediaParserError::Mp4(Mp4Error::Error {
            message: "stts box not found in stbl box".to_string(),
        })
    })?;

    if stts.len() < 8 {
        return Err(MediaParserError::Mp4(Mp4Error::Error {
            message: "stts box too small: expected at least 8 bytes".to_string(),
        }));
    }

    let entry_count = u32::from_be_bytes([stts[4], stts[5], stts[6], stts[7]]);

    // Verify that the box has enough space for all entries
    let required_size = 8 + (entry_count as usize * 8);
    if required_size > stts.len() {
        return Err(MediaParserError::Mp4(Mp4Error::Error {
            message: format!(
                "stts box too small for {} entries: expected {} bytes, got {}",
                entry_count,
                required_size,
                stts.len()
            ),
        }));
    }

    let mut entries = Vec::with_capacity(entry_count as usize);

    for i in 0..entry_count {
        let entry_pos = 8 + (i * 8) as usize;
        let sample_count = u32::from_be_bytes([
            stts[entry_pos],
            stts[entry_pos + 1],
            stts[entry_pos + 2],
            stts[entry_pos + 3],
        ]);
        let sample_delta = u32::from_be_bytes([
            stts[entry_pos + 4],
            stts[entry_pos + 5],
            stts[entry_pos + 6],
            stts[entry_pos + 7],
        ]);

        entries.push(SttsEntry {
            sample_count,
            sample_delta,
        });
    }

    Ok(entries)
}

// Parse stts for thumbnails (strict error handling)
alias_strict!(parse_stts_thumbnails, parse_stts, Vec<SttsEntry>);
alias_lenient!(parse_stts_subtitles, parse_stts, Vec<SttsEntry>);
alias_lenient!(parse_stts_lenient, parse_stts, Vec<SttsEntry>);

/// Build sample timestamps (seconds) from STTS entries
pub fn build_sample_timestamps(timescale: u32, entries: &[SttsEntry]) -> Vec<f64> {
    let mut timestamps = Vec::new();
    let mut time_offset = 0u64;

    for entry in entries {
        for _ in 0..entry.sample_count {
            timestamps.push(time_offset as f64 / timescale as f64);
            time_offset += entry.sample_delta as u64;
        }
    }

    timestamps
}
