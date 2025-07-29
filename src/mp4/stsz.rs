use super::r#box::find_box;
use crate::errors::{MediaParserError, MediaParserResult, Mp4Error};

/// Parse stsz (sample size) box - unified function
pub fn parse_stsz(stbl: &[u8]) -> MediaParserResult<Vec<u32>> {
    let stsz = find_box(stbl, "stsz").ok_or_else(|| {
        MediaParserError::Mp4(Mp4Error::Error {
            message: "stsz box not found in stbl box".to_string(),
        })
    })?;

    if stsz.len() < 12 {
        return Err(MediaParserError::Mp4(Mp4Error::Error {
            message: "stsz box too small: expected at least 12 bytes".to_string(),
        }));
    }

    let sample_size = u32::from_be_bytes([stsz[4], stsz[5], stsz[6], stsz[7]]);
    let sample_count = u32::from_be_bytes([stsz[8], stsz[9], stsz[10], stsz[11]]);

    if sample_size != 0 {
        // All samples have the same size
        Ok(vec![sample_size; sample_count as usize])
    } else {
        // Individual sample sizes
        let required_size = 12 + (sample_count as usize * 4);
        if required_size > stsz.len() {
            return Err(MediaParserError::Mp4(Mp4Error::Error {
                message: format!(
                    "stsz box too small for {} samples: expected {} bytes, got {}",
                    sample_count,
                    required_size,
                    stsz.len()
                ),
            }));
        }

        let mut sizes = Vec::with_capacity(sample_count as usize);
        for i in 0..sample_count {
            let size_pos = 12 + (i * 4) as usize;
            let size = u32::from_be_bytes([
                stsz[size_pos],
                stsz[size_pos + 1],
                stsz[size_pos + 2],
                stsz[size_pos + 3],
            ]);
            sizes.push(size);
        }
        Ok(sizes)
    }
}

alias_strict!(parse_stsz_thumbnails, parse_stsz, Vec<u32>);
alias_lenient!(parse_stsz_subtitles, parse_stsz, Vec<u32>);
alias_lenient!(parse_stsz_lenient, parse_stsz, Vec<u32>);
