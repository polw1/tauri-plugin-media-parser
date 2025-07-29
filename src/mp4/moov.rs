use super::r#box::find_box;
use crate::errors::MediaParserResult;
use crate::metadata::{ContainerFormat, Metadata};
use crate::mp4::mvhd::extract_duration_from_mvhd;
use crate::mp4::trak::extract_stream_info_from_trak;
use crate::mp4::udta::{extract_tags_from_udta, extract_title_from_udta};

/// Extract basic metadata from moov box data
pub fn extract_mp4_metadata_from_moov(
    moov_data: &[u8],
    file_size: u64,
    format: ContainerFormat,
) -> MediaParserResult<Metadata> {
    let mut metadata = Metadata {
        title: None,
        artist: None,
        album: None,
        copyright: None,
        format: Some(format),
        duration: None,
        size: file_size,
        streams: Vec::new(),
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
                metadata.streams.push(stream_info);
                track_index += 1;
            }
        }

        pos += box_size;
    }

    Ok(metadata)
}

/// Legacy function for backward compatibility - uses the new modular approach
pub fn parse_moov(
    data: &[u8],
    title: &mut Option<String>,
    artist: &mut Option<String>,
    album: &mut Option<String>,
    copyright: &mut Option<String>,
    duration: &mut Option<f64>,
) -> MediaParserResult<()> {
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
            streams: Vec::new(),
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
