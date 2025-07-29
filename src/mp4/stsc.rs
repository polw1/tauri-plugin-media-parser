use super::r#box::find_box;
use crate::errors::{MediaParserError, MediaParserResult, Mp4Error};

#[derive(Debug, Clone, PartialEq)]
pub struct SampleToChunkEntry {
    pub first_chunk: u32,
    pub samples_per_chunk: u32,
    pub sample_description_index: u32,
}

/// Parse stsc (sample to chunk) box - unified function
pub fn parse_stsc(stbl: &[u8]) -> MediaParserResult<Vec<SampleToChunkEntry>> {
    let stsc = find_box(stbl, "stsc").ok_or_else(|| {
        MediaParserError::Mp4(Mp4Error::Error {
            message: "stsc box not found in stbl box".to_string(),
        })
    })?;

    if stsc.len() < 8 {
        return Err(MediaParserError::Mp4(Mp4Error::Error {
            message: "stsc box too small: expected at least 8 bytes".to_string(),
        }));
    }

    let entry_count = u32::from_be_bytes([stsc[4], stsc[5], stsc[6], stsc[7]]);

    // Verify that the box has enough space for all entries
    let required_size = 8 + (entry_count as usize * 12);
    if required_size > stsc.len() {
        return Err(MediaParserError::Mp4(Mp4Error::Error {
            message: format!(
                "stsc box too small for {} entries: expected {} bytes, got {}",
                entry_count,
                required_size,
                stsc.len()
            ),
        }));
    }

    let mut entries = Vec::with_capacity(entry_count as usize);

    for i in 0..entry_count {
        let entry_pos = 8 + (i * 12) as usize;
        let first_chunk = u32::from_be_bytes([
            stsc[entry_pos],
            stsc[entry_pos + 1],
            stsc[entry_pos + 2],
            stsc[entry_pos + 3],
        ]);
        let samples_per_chunk = u32::from_be_bytes([
            stsc[entry_pos + 4],
            stsc[entry_pos + 5],
            stsc[entry_pos + 6],
            stsc[entry_pos + 7],
        ]);
        let sample_description_index = u32::from_be_bytes([
            stsc[entry_pos + 8],
            stsc[entry_pos + 9],
            stsc[entry_pos + 10],
            stsc[entry_pos + 11],
        ]);

        entries.push(SampleToChunkEntry {
            first_chunk,
            samples_per_chunk,
            sample_description_index,
        });
    }

    Ok(entries)
}

// Parse stsc for thumbnails (strict error handling)
alias_strict!(parse_stsc_thumbnails, parse_stsc, Vec<SampleToChunkEntry>);
alias_lenient!(parse_stsc_subtitles, parse_stsc, Vec<SampleToChunkEntry>);
alias_lenient!(parse_stsc_lenient, parse_stsc, Vec<SampleToChunkEntry>);
