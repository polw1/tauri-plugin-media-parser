/// Extract codec and details from stsd box
type StsdDetails = (String, Option<f64>, Option<u32>, Option<u32>, Option<u16>);

/// Extract details from stsd function.
pub fn extract_details_from_stsd(stsd: &[u8], track_kind: &str) -> Option<StsdDetails> {
    if stsd.len() < 8 {
        return None;
    }

    // Skip version and flags (4 bytes) and entry count (4 bytes)
    let mut pos = 8;

    if pos + 8 > stsd.len() {
        return None;
    }

    // Read first sample description entry
    let _entry_size =
        u32::from_be_bytes([stsd[pos], stsd[pos + 1], stsd[pos + 2], stsd[pos + 3]]) as usize;
    pos += 4;

    if pos + 4 > stsd.len() {
        return None;
    }

    // Read codec fourCC
    let codec_fourcc = std::str::from_utf8(&stsd[pos..pos + 4]).unwrap_or("unknown");
    pos += 4;

    let mut codec_id = codec_fourcc.to_string();
    let mut width = None;
    let mut height = None;
    let mut channels = None;
    let frame_rate = None;

    match track_kind {
        "video" => {
            // Video sample description requires 28 bytes after fourCC
            if pos + 28 <= stsd.len() {
                // Skip reserved fields (6 bytes) and data reference index (2 bytes)
                pos += 8;
                // Skip version and revision level (4 bytes)
                pos += 4;
                // Skip vendor (4 bytes)
                pos += 4;
                // Skip temporal quality and spatial quality (8 bytes)
                pos += 8;

                // Read width and height (2 bytes each)
                width = Some(u16::from_be_bytes([stsd[pos], stsd[pos + 1]]) as u32);
                pos += 2;
                height = Some(u16::from_be_bytes([stsd[pos], stsd[pos + 1]]) as u32);
                // Not advancing further after reading height
            }

            // Map common video codecs
            codec_id = match codec_fourcc {
                "avc1" | "avc3" => "H.264/AVC".to_string(),
                "hev1" | "hvc1" => "H.265/HEVC".to_string(),
                "mp4v" => "MPEG-4 Visual".to_string(),
                "av01" => "AV1".to_string(),
                _ => codec_fourcc.to_string(),
            };
        }
        "audio" => {
            // Audio sample description requires 18 bytes after fourCC
            if pos + 18 <= stsd.len() {
                // Skip reserved fields (6 bytes) and data reference index (2 bytes)
                pos += 8;
                // Skip version and revision level (4 bytes)
                pos += 4;
                // Skip vendor (4 bytes)
                pos += 4;

                // Read channel count (2 bytes)
                channels = Some(u16::from_be_bytes([stsd[pos], stsd[pos + 1]]));
                // Not advancing further after reading channels
            }

            // Map common audio codecs
            codec_id = match codec_fourcc {
                "mp4a" => "AAC".to_string(),
                "ac-3" => "AC-3".to_string(),
                "ec-3" => "E-AC-3".to_string(),
                "Opus" => "Opus".to_string(),
                _ => codec_fourcc.to_string(),
            };
        }
        "subtitle" => {
            // Map subtitle codecs
            codec_id = match codec_fourcc {
                "tx3g" => "3GPP Timed Text".to_string(),
                "wvtt" => "WebVTT".to_string(),
                "stpp" => "XML Subtitle".to_string(),
                _ => codec_fourcc.to_string(),
            };
        }
        _ => {}
    }

    Some((codec_id, frame_rate, width, height, channels))
}

#[cfg(test)]
mod tests {
    use crate::mp4::stsd::*;
    #[test]
    fn test_extract_details_from_stsd() {
        let stsd_data = [
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, // version, flags, entry count
            0x00, 0x00, 0x00, 0x1f, // entry size (31)
            b'a', b'v', b'c', b'1', // codec fourCC
            // Reserved + data reference index
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, // Version + revision
            0x00, 0x00, 0x00, 0x00, // Vendor
            0x00, 0x00, 0x00, 0x00, // Temporal quality + spatial quality
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // Width (320) + Height (240)
            0x01, 0x40, 0x00, 0xF0,
        ];
        let (codec_id, frame_rate, width, height, channels) =
            extract_details_from_stsd(&stsd_data, "video").expect("Should parse stsd details");
        assert_eq!(codec_id, "H.264/AVC");
        assert_eq!(frame_rate, None);
        assert_eq!(width, Some(320));
        assert_eq!(height, Some(240));
        assert_eq!(channels, None);
    }
}
