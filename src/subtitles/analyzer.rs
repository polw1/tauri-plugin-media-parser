use super::types::SubtitleTrackInfo;
use crate::errors::MediaParserResult;
use crate::mp4::r#box::{find_box, find_box_range};
use crate::mp4::stco::parse_stco_or_co64_subtitles;
use crate::mp4::stsc::parse_stsc_subtitles;
use crate::mp4::stsz::parse_stsz_subtitles;
use crate::mp4::stts::parse_stts_subtitles;
use log::{debug, info, warn};

/// Analyze subtitle tracks from moov payload
pub(crate) fn analyze_subtitle_tracks(
    moov_payload: &[u8],
) -> MediaParserResult<Vec<SubtitleTrackInfo>> {
    let mut tracks = Vec::new();
    let mut pos = 0;
    let mut track_count = 0;

    info!("Analyzing moov payload ({} bytes)", moov_payload.len());

    // Find all trak boxes with safety limits
    while pos < moov_payload.len() && track_count < 50 {
        // Safety check to prevent infinite loops
        if pos + 8 > moov_payload.len() {
            break;
        }

        let box_size = u32::from_be_bytes([
            moov_payload[pos],
            moov_payload[pos + 1],
            moov_payload[pos + 2],
            moov_payload[pos + 3],
        ]) as usize;

        let box_type = std::str::from_utf8(&moov_payload[pos + 4..pos + 8]).unwrap_or("????");

        // Only process trak boxes
        if box_type == "trak" {
            track_count += 1;
            debug!(
                "  Found trak box #{} at position {} (size: {})",
                track_count, pos, box_size
            );

            if pos + box_size > moov_payload.len() {
                warn!("    Trak box extends beyond payload, skipping");
                break;
            }

            let trak_data = &moov_payload[pos..pos + box_size];

            // Check if this is a subtitle track
            if is_subtitle_track(trak_data) {
                debug!("    This is a subtitle track");
                if let Some(track_info) = parse_subtitle_track_info(trak_data) {
                    tracks.push(track_info);
                }
            } else {
                debug!("    Not a subtitle track");
            }
        }

        // Move to next box
        if box_size == 0 || box_size < 8 {
            break;
        }

        pos += box_size;

        // Safety check
        if pos > moov_payload.len() {
            break;
        }
    }

    if track_count >= 50 {
        warn!("Reached maximum track limit (50), stopping analysis");
    }

    info!("Found {} subtitle tracks", tracks.len());
    Ok(tracks)
}

/// Check if a track is a subtitle track
pub(crate) fn is_subtitle_track(trak_data: &[u8]) -> bool {
    let trak_payload = &trak_data[8..];

    if let Some((_, mdia_start, mdia_end)) = find_box_range(trak_payload, "mdia") {
        let mdia_data = &trak_payload[mdia_start..mdia_end];

        if let Some((_, hdlr_start, hdlr_end)) = find_box_range(mdia_data, "hdlr") {
            let hdlr_data = &mdia_data[hdlr_start..hdlr_end];

            if hdlr_data.len() >= 12 {
                let handler_type = std::str::from_utf8(&hdlr_data[8..12]).unwrap_or("????");
                debug!("    Handler type found: '{}'", handler_type);
                if handler_type == "subt" || handler_type == "text" || handler_type == "sbtl" {
                    return true;
                }
            }
        }

        if let Some((_, minf_start, minf_end)) = find_box_range(mdia_data, "minf") {
            let minf_data = &mdia_data[minf_start..minf_end];

            if find_box(minf_data, "sbtl").is_some()
                || find_box(minf_data, "subt").is_some()
                || find_box(minf_data, "text").is_some()
            {
                return true;
            }
        }
    }

    false
}

/// Parse subtitle track information from trak box
pub(crate) fn parse_subtitle_track_info(trak_data: &[u8]) -> Option<SubtitleTrackInfo> {
    let trak_payload = &trak_data[8..];

    let track_id = if let Some((_, tkhd_start, tkhd_end)) = find_box_range(trak_payload, "tkhd") {
        let tkhd_data = &trak_payload[tkhd_start..tkhd_end];
        if tkhd_data.len() >= 8 {
            // Track ID is at offset 4 in tkhd payload
            u32::from_be_bytes([tkhd_data[4], tkhd_data[5], tkhd_data[6], tkhd_data[7]])
        } else {
            return None;
        }
    } else {
        return None;
    };

    let (_, mdia_start, mdia_end) = find_box_range(trak_payload, "mdia")?;
    let mdia_data = &trak_payload[mdia_start..mdia_end];

    // Get timescale from mdhd
    let timescale = if let Some((_, mdhd_payload_start, _)) = find_box_range(mdia_data, "mdhd") {
        let mdhd_payload = &mdia_data[mdhd_payload_start..];
        if !mdhd_payload.is_empty() {
            let version = mdhd_payload[0];
            let timescale_offset = if version == 1 {
                20 // 4 (version/flags) + 8 (creation_time) + 8 (modification_time)
            } else {
                12 // 4 (version/flags) + 4 (creation_time) + 4 (modification_time)
            };

            if mdhd_payload.len() >= timescale_offset + 4 {
                let ts = u32::from_be_bytes([
                    mdhd_payload[timescale_offset],
                    mdhd_payload[timescale_offset + 1],
                    mdhd_payload[timescale_offset + 2],
                    mdhd_payload[timescale_offset + 3],
                ]);
                debug!("    mdhd version: {}, timescale: {}", version, ts);
                ts
            } else {
                1000 // Default
            }
        } else {
            1000 // Default
        }
    } else {
        1000 // Default timescale
    };

    let (_, minf_start, minf_end) = find_box_range(mdia_data, "minf")?;
    let minf_data = &mdia_data[minf_start..minf_end];

    let (_, stbl_start, stbl_end) = find_box_range(minf_data, "stbl")?;
    let stbl_data = &minf_data[stbl_start..stbl_end];

    let chunk_offsets = parse_stco_or_co64_subtitles(stbl_data);
    let sample_sizes = parse_stsz_subtitles(stbl_data);
    let sample_to_chunk = parse_stsc_subtitles(stbl_data);
    let stts_entries = parse_stts_subtitles(stbl_data);

    let codec_type = determine_subtitle_codec(stbl_data);

    Some(SubtitleTrackInfo {
        _track_id: track_id,
        timescale,
        chunk_offsets,
        sample_sizes,
        sample_to_chunk,
        stts_entries,
        codec_type,
    })
}

/// Determine subtitle codec type from stbl data
fn determine_subtitle_codec(stbl_data: &[u8]) -> String {
    if let Some((_, stsd_start, stsd_end)) = find_box_range(stbl_data, "stsd") {
        let stsd_data = &stbl_data[stsd_start..stsd_end];
        if stsd_data.len() >= 16 {
            // Skip version/flags (4 bytes) and entry count (4 bytes)
            let entry_start = 8;
            if stsd_data.len() >= entry_start + 8 {
                // Sample description entry: size (4) + format (4) + ...
                let format_bytes = &stsd_data[entry_start + 4..entry_start + 8];
                if let Ok(codec) = String::from_utf8(format_bytes.to_vec()) {
                    let cleaned = codec.trim_end_matches('\0').trim().to_string();
                    if !cleaned.is_empty() && cleaned.chars().all(|c| c.is_ascii_graphic()) {
                        return cleaned;
                    }
                }
            }
        }
    }

    "text".to_string() // Default fallback
}
