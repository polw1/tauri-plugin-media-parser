use super::types::SubtitleEntry;
use super::utils::format_timestamp;
use std::io;

/// Parse subtitle sample data based on codec type
pub fn parse_subtitle_sample_data(
    data: &[u8],
    timestamp: f64,
    codec_type: &str,
) -> io::Result<Vec<SubtitleEntry>> {
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
                "⚠️  Unknown subtitle codec: {}, trying generic parser",
                codec_type
            );
            parse_generic_subtitle(data, timestamp)
        }
    }
}

/// Parse TX3G (3GPP Timed Text) subtitle format
fn parse_tx3g_subtitle(data: &[u8], timestamp: f64) -> io::Result<Vec<SubtitleEntry>> {
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
fn parse_webvtt_subtitle(data: &[u8], timestamp: f64) -> io::Result<Vec<SubtitleEntry>> {
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
fn parse_ttml_subtitle(data: &[u8], timestamp: f64) -> io::Result<Vec<SubtitleEntry>> {
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
fn parse_generic_subtitle(data: &[u8], timestamp: f64) -> io::Result<Vec<SubtitleEntry>> {
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
