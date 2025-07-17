use super::r#box::find_box;
use std::io;

#[derive(Debug, Clone, PartialEq)]
pub struct SampleToChunkEntry {
    pub first_chunk: u32,
    pub samples_per_chunk: u32,
    pub sample_description_index: u32,
}

/// Parse stsc (sample to chunk) box - unified function
pub fn parse_stsc(stbl: &[u8]) -> io::Result<Vec<SampleToChunkEntry>> {
    let stsc = find_box(stbl, "stsc")
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "stsc box not found"))?;

    if stsc.len() < 8 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "stsc box too small",
        ));
    }

    let entry_count = u32::from_be_bytes([stsc[4], stsc[5], stsc[6], stsc[7]]);
    let mut entries = Vec::new();

    for i in 0..entry_count {
        let entry_pos = 8 + (i * 12) as usize;
        if entry_pos + 12 <= stsc.len() {
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
    }

    Ok(entries)
}

// Parse stsc for thumbnails (strict error handling)
alias_strict!(parse_stsc_thumbnails, parse_stsc, Vec<SampleToChunkEntry>);
alias_lenient!(parse_stsc_subtitles, parse_stsc, Vec<SampleToChunkEntry>);
alias_lenient!(parse_stsc_lenient, parse_stsc, Vec<SampleToChunkEntry>);
