use super::types::SubtitleEntry;
use super::utils::format_timestamp;
use crate::errors::MediaParserResult;
#[cfg(test)]
use std::io;

/// Parse subtitle sample data based on codec type
pub fn parse_subtitle_sample_data(
    data: &[u8],
    timestamp: f64,
    codec_type: &str,
) -> MediaParserResult<Vec<SubtitleEntry>> {
    if data.is_empty() {
        return Ok(Vec::new());
    }

    match codec_type {
        "tx3g" => parse_tx3g_subtitle(data, timestamp),
        "wvtt" => parse_webvtt_subtitle(data, timestamp),
        "stpp" => parse_ttml_subtitle(data, timestamp),
        "sbtl" | "subt" => parse_generic_subtitle(data, timestamp),
        _ => {
            println!(
                "Unknown subtitle codec: {}, trying generic parser",
                codec_type
            );
            parse_generic_subtitle(data, timestamp)
        }
    }
}

/// Parse TX3G (3GPP Timed Text) subtitle format
fn parse_tx3g_subtitle(data: &[u8], timestamp: f64) -> MediaParserResult<Vec<SubtitleEntry>> {
    if data.len() < 2 {
        return Ok(Vec::new());
    }

    // TX3G format: 2-byte text length + text data
    let text_length = u16::from_be_bytes([data[0], data[1]]) as usize;

    if text_length == 0 || data.len() < 2 + text_length {
        return Ok(Vec::new());
    }

    let text_data = &data[2..2 + text_length];

    // Try to decode as UTF-8
    if let Ok(text) = String::from_utf8(text_data.to_vec()) {
        if !text.trim().is_empty() {
            return Ok(vec![SubtitleEntry {
                start: format_timestamp(timestamp),
                end: format_timestamp(timestamp + 2.0), // Default 2-second duration
                text: text.trim().to_string(),
            }]);
        }
    }

    Ok(Vec::new())
}

/// Parse WebVTT subtitle format
fn parse_webvtt_subtitle(data: &[u8], timestamp: f64) -> MediaParserResult<Vec<SubtitleEntry>> {
    if let Ok(text) = String::from_utf8(data.to_vec()) {
        let trimmed = text.trim();
        if !trimmed.is_empty() && !trimmed.starts_with("WEBVTT") {
            return Ok(vec![SubtitleEntry {
                start: format_timestamp(timestamp),
                end: format_timestamp(timestamp + 2.0), // Default 2-second duration
                text: trimmed.to_string(),
            }]);
        }
    }

    Ok(Vec::new())
}

/// Parse TTML subtitle format
fn parse_ttml_subtitle(data: &[u8], timestamp: f64) -> MediaParserResult<Vec<SubtitleEntry>> {
    if let Ok(text) = String::from_utf8(data.to_vec()) {
        // Simple TTML parsing - extract text content between tags
        let mut result = String::new();
        let mut in_tag = false;

        for ch in text.chars() {
            match ch {
                '<' => in_tag = true,
                '>' => in_tag = false,
                _ if !in_tag => result.push(ch),
                _ => {}
            }
        }

        let trimmed = result.trim();
        if !trimmed.is_empty() {
            return Ok(vec![SubtitleEntry {
                start: format_timestamp(timestamp),
                end: format_timestamp(timestamp + 2.0), // Default 2-second duration
                text: trimmed.to_string(),
            }]);
        }
    }

    Ok(Vec::new())
}

/// Parse generic subtitle format (fallback)
fn parse_generic_subtitle(data: &[u8], timestamp: f64) -> MediaParserResult<Vec<SubtitleEntry>> {
    // Try UTF-8 first
    if let Ok(text) = String::from_utf8(data.to_vec()) {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            return Ok(vec![SubtitleEntry {
                start: format_timestamp(timestamp),
                end: format_timestamp(timestamp + 2.0), // Default 2-second duration
                text: trimmed.to_string(),
            }]);
        }
    }

    // Try UTF-16 if UTF-8 fails
    if data.len() >= 2 && data.len() % 2 == 0 {
        let mut utf16_chars = Vec::new();
        for i in (0..data.len()).step_by(2) {
            let char_code = u16::from_be_bytes([data[i], data[i + 1]]);
            utf16_chars.push(char_code);
        }

        if let Ok(text) = String::from_utf16(&utf16_chars) {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                return Ok(vec![SubtitleEntry {
                    start: format_timestamp(timestamp),
                    end: format_timestamp(timestamp + 2.0), // Default 2-second duration
                    text: trimmed.to_string(),
                }]);
            }
        }
    }

    Ok(Vec::new())
}

#[cfg(test)]
mod test_helpers {
    pub const TX3G_SAMPLE_1: [u8; 32] = [
        0x00, 0x1e, 0x53, 0x65, 0x72, 0xc3, 0xa1, 0x20, 0x71, 0x75, 0x65, 0x20, 0x76, 0x6f, 0x63,
        0xc3, 0xaa, 0x20, 0x66, 0x6f, 0x69, 0x20, 0x69, 0x6e, 0x66, 0x65, 0x63, 0x74, 0x61, 0x64,
        0x6f, 0x3f,
    ];
    pub const TX3G_SAMPLE_2: [u8; 44] = [
        0x00, 0x2a, 0x4e, 0xc3, 0xa3, 0x6f, 0x2c, 0x20, 0x6e, 0xc3, 0xa3, 0x6f, 0x2c, 0x0a, 0x6e,
        0xc3, 0xa3, 0x6f, 0x20, 0x63, 0x6f, 0x6d, 0x20, 0x75, 0x6d, 0x20, 0x76, 0xc3, 0xad, 0x72,
        0x75, 0x73, 0x20, 0x64, 0x65, 0x20, 0x76, 0x65, 0x72, 0x64, 0x61, 0x64, 0x65, 0x2c,
    ];
    /// Mock tx3g sample1 function.
    pub fn mock_tx3g_sample1() -> Vec<u8> {
        TX3G_SAMPLE_1.to_vec()
    }
    /// Mock tx3g sample2 function.
    pub fn mock_tx3g_sample2() -> Vec<u8> {
        TX3G_SAMPLE_2.to_vec()
    }
}

#[test]
fn test_parse_tx3g_samples() -> io::Result<()> {
    use test_helpers::*;
    let sample1 = mock_tx3g_sample1();
    let entries1 = parse_subtitle_sample_data(&sample1, 4.693, "tx3g")?;
    assert_eq!(entries1.len(), 1);
    assert_eq!(entries1[0].start, format_timestamp(4.693));
    assert_eq!(entries1[0].end, format_timestamp(4.693 + 2.0));
    assert_eq!(entries1[0].text, "Será que você foi infectado?");
    let sample2 = mock_tx3g_sample2();
    let entries2 = parse_subtitle_sample_data(&sample2, 7.238, "tx3g")?;
    assert_eq!(entries2.len(), 1);
    assert_eq!(entries2[0].start, format_timestamp(7.238));
    assert_eq!(entries2[0].text, "Não, não,\nnão com um vírus de verdade,");
    Ok(())
}

#[test]
fn test_subtitle_error_handling() {
    let empty = Vec::<u8>::new();
    let entries = parse_subtitle_sample_data(&empty, 0.0, "tx3g").unwrap();
    assert!(entries.is_empty());

    let generic = parse_subtitle_sample_data(b"Hello", 1.0, "unknown").unwrap();
    assert_eq!(generic.len(), 1);
    assert_eq!(generic[0].text, "Hello");
}

#[test]
fn test_parse_wvtt_and_stpp_samples() -> io::Result<()> {
    let wvtt = b"Hello WebVTT";
    let entries = parse_subtitle_sample_data(wvtt, 1.0, "wvtt")?;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].text, "Hello WebVTT");

    let ttml = b"<p>Caption</p>";
    let entries2 = parse_subtitle_sample_data(ttml, 2.0, "stpp")?;
    assert_eq!(entries2.len(), 1);
    assert_eq!(entries2[0].text, "Caption");
    Ok(())
}
