use crate::subtitles::{format_timestamp, parse_subtitle_sample_data};
use std::io;

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
    pub fn mock_tx3g_sample1() -> Vec<u8> {
        TX3G_SAMPLE_1.to_vec()
    }
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
