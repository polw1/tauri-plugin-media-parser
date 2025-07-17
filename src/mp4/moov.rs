use super::r#box::find_box;
use crate::metadata::{CompleteMetadata, ContainerFormat, Metadata};
use crate::mp4::mvhd::extract_duration_from_mvhd;
use crate::mp4::trak::extract_stream_info_from_trak;
use crate::mp4::udta::{extract_tags_from_udta, extract_title_from_udta};
use std::io;

/// Extract basic metadata from moov box data
pub fn extract_mp4_metadata_from_moov(
    moov_data: &[u8],
    file_size: u64,
    format: ContainerFormat,
) -> io::Result<Metadata> {
    let mut metadata = Metadata {
        title: None,
        artist: None,
        album: None,
        copyright: None,
        format: Some(format),
        duration: None,
        size: file_size,
    };

    // Look for mvhd box for duration
    if let Some(mvhd) = find_box(moov_data, "mvhd") {
        if let Some(duration) = extract_duration_from_mvhd(mvhd) {
            metadata.duration = Some(duration);
        }
    }

    // Look for udta box for metadata tags
    if let Some(udta) = find_box(moov_data, "udta") {
        extract_tags_from_udta(udta, &mut metadata);
        // Also try dedicated title extraction for better results
        if metadata.title.is_none() {
            metadata.title = extract_title_from_udta(udta);
        }
    }

    Ok(metadata)
}

/// Extract complete metadata from moov box data
pub fn extract_complete_mp4_metadata_from_moov(
    moov_data: &[u8],
    format: ContainerFormat,
) -> io::Result<CompleteMetadata> {
    let mut duration = 0.0;
    let mut title = None;
    let mut streams = Vec::new();

    // Look for mvhd box for duration
    if let Some(mvhd) = find_box(moov_data, "mvhd") {
        if let Some(d) = extract_duration_from_mvhd(mvhd) {
            duration = d;
        }
    }

    // Look for udta box for title
    if let Some(udta) = find_box(moov_data, "udta") {
        title = extract_title_from_udta(udta);
    }

    // Parse tracks for stream information
    let mut track_index = 0;
    let mut pos = 0;
    while pos + 8 <= moov_data.len() {
        let box_size = u32::from_be_bytes([
            moov_data[pos],
            moov_data[pos + 1],
            moov_data[pos + 2],
            moov_data[pos + 3],
        ]) as usize;

        if box_size < 8 || pos + box_size > moov_data.len() {
            break;
        }

        let box_type = &moov_data[pos + 4..pos + 8];
        if box_type == b"trak" {
            let trak_data = &moov_data[pos + 8..pos + box_size];
            if let Some(stream_info) = extract_stream_info_from_trak(trak_data, track_index) {
                streams.push(stream_info);
                track_index += 1;
            }
        }

        pos += box_size;
    }

    Ok(CompleteMetadata {
        duration,
        title,
        streams,
        format,
    })
}

// Legacy function for backward compatibility - uses the new modular approach
pub fn parse_moov(
    data: &[u8],
    title: &mut Option<String>,
    artist: &mut Option<String>,
    album: &mut Option<String>,
    copyright: &mut Option<String>,
    duration: &mut Option<f64>,
) -> io::Result<()> {
    // Use the new modular approach
    if let Some(mvhd) = find_box(data, "mvhd") {
        if let Some(d) = extract_duration_from_mvhd(mvhd) {
            *duration = Some(d);
        }
    }

    if let Some(udta) = find_box(data, "udta") {
        // Create a temporary metadata struct to use the modular function
        let mut temp_metadata = Metadata {
            title: None,
            artist: None,
            album: None,
            copyright: None,
            format: None,
            duration: None,
            size: 0,
        };

        extract_tags_from_udta(udta, &mut temp_metadata);

        // Copy results back to the provided parameters
        *title = temp_metadata.title;
        *artist = temp_metadata.artist;
        *album = temp_metadata.album;
        *copyright = temp_metadata.copyright;
    }

    Ok(())
}
