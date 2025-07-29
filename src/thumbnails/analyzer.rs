use super::types::VideoTrackInfo;
use crate::errors::{MediaParserResult, ThumbnailError};
use crate::mp4::mdhd::parse_mdhd;
use crate::mp4::r#box::{find_box, parse_box_header};
use crate::mp4::stco::parse_stco_or_co64_thumbnails;
use crate::mp4::stsc::parse_stsc_thumbnails;
use crate::mp4::stss::parse_stss_thumbnails;
use crate::mp4::stsz::parse_stsz_thumbnails;
use crate::mp4::stts::parse_stts_thumbnails;
use crate::mp4::AvccConfig;

/// Analyze the video track from moov payload to extract all timing and location information
pub(crate) fn analyze_video_track(moov_payload: &[u8]) -> MediaParserResult<VideoTrackInfo> {
    // Find the first video track
    let video_trak =
        find_video_trak(moov_payload).ok_or_else(|| ThumbnailError::new("No video track"))?;

    let mdia = find_box(video_trak, "mdia").ok_or_else(|| ThumbnailError::new("No mdia box"))?;

    let mdhd = find_box(mdia, "mdhd").ok_or_else(|| ThumbnailError::new("No mdhd box"))?;

    let minf = find_box(mdia, "minf").ok_or_else(|| ThumbnailError::new("No minf box"))?;

    let stbl = find_box(minf, "stbl").ok_or_else(|| ThumbnailError::new("No stbl box"))?;

    // Extract timing information from mdhd
    let (timescale, duration) = parse_mdhd(mdhd)?;

    // Extract sample information from stbl
    let chunk_offsets = parse_stco_or_co64_thumbnails(stbl)?;
    let sample_sizes = parse_stsz_thumbnails(stbl)?;
    let sample_to_chunk = parse_stsc_thumbnails(stbl)?;
    let stts_entries = parse_stts_thumbnails(stbl)?;
    let stss_entries = parse_stss_thumbnails(stbl).unwrap_or_default(); // Sync samples (optional)

    // Extract avcC configuration from sample description
    let avcc = if let Some(stsd) = find_box(stbl, "stsd") {
        extract_avcc_from_stsd(stsd)
    } else {
        None
    };

    let sample_count = sample_sizes.len() as u32;

    Ok(VideoTrackInfo {
        timescale,
        _duration: duration,
        sample_count,
        chunk_offsets,
        sample_sizes,
        sample_to_chunk,
        stts_entries,
        stss_entries,
        avcc,
    })
}

/// Find the first video track in moov payload
fn find_video_trak(moov_payload: &[u8]) -> Option<&[u8]> {
    let mut pos = 0usize;

    while pos + 8 <= moov_payload.len() {
        let start = pos;
        if let Some((name, size)) = parse_box_header(moov_payload, &mut pos) {
            if size as usize > moov_payload.len() - start {
                break;
            }
            let payload = &moov_payload[pos..start + size as usize];

            if name == "trak" {
                // Check if this is a video track
                if let Some(mdia) = find_box(payload, "mdia") {
                    if let Some(hdlr) = find_box(mdia, "hdlr") {
                        if hdlr.len() >= 16 && &hdlr[8..12] == b"vide" {
                            return Some(payload);
                        }
                    }
                }
            }
            pos = start + size as usize;
        } else {
            break;
        }
    }
    None
}

/// Extract AVCC configuration from stsd box
fn extract_avcc_from_stsd(stsd: &[u8]) -> Option<AvccConfig> {
    if stsd.len() < 8 {
        return None;
    }

    let entry_count = u32::from_be_bytes([stsd[4], stsd[5], stsd[6], stsd[7]]);
    let mut pos = 8; // Skip header + entry_count

    for _ in 0..entry_count {
        if pos + 8 > stsd.len() {
            break;
        }

        let entry_size =
            u32::from_be_bytes([stsd[pos], stsd[pos + 1], stsd[pos + 2], stsd[pos + 3]]) as usize;
        if pos + entry_size > stsd.len() {
            break;
        }

        let entry_data = &stsd[pos..pos + entry_size];

        // Check if this is an AVC entry (avc1 or avc3)
        if entry_data.len() >= 8 {
            let codec_type = &entry_data[4..8];
            if codec_type == b"avc1" || codec_type == b"avc3" {
                // Search for avcC within this entry
                if let Some(avcc_config) = search_avcc_in_entry(entry_data) {
                    return Some(avcc_config);
                }
            }
        }

        pos += entry_size;
    }

    None
}

/// Search for avcC box within a sample entry
fn search_avcc_in_entry(entry_data: &[u8]) -> Option<AvccConfig> {
    // Skip the sample entry header and video-specific fields
    let mut pos = 8 + 6 + 2 + 70; // size+type + reserved + data_ref + video fields

    while pos + 8 <= entry_data.len() {
        let start = pos;
        if let Some((name, size)) = parse_box_header(entry_data, &mut pos) {
            if size as usize > entry_data.len() - start {
                break;
            }
            let payload = &entry_data[pos..start + size as usize];

            if name == "avcC" {
                // Found avcC box, try to parse it
                if let Ok(config) = AvccConfig::parse(payload) {
                    return Some(config);
                }
            }

            pos = start + size as usize;
        } else {
            break;
        }
    }

    None
}
