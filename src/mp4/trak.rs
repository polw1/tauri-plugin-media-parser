use crate::metadata::StreamInfo;
use crate::mp4::mdhd::extract_language_from_mdhd;
use crate::mp4::r#box::find_box;
use crate::mp4::stsd::extract_details_from_stsd;

/// Extract stream info from trak box
pub fn extract_stream_info_from_trak(trak_data: &[u8], index: usize) -> Option<StreamInfo> {
    // Look for mdia box
    let mdia = find_box(trak_data, "mdia")?;

    // Look for hdlr box to determine track type
    let hdlr = find_box(mdia, "hdlr")?;
    if hdlr.len() < 12 {
        return None;
    }

    let handler_type = std::str::from_utf8(&hdlr[8..12]).ok()?;
    let kind = match handler_type {
        "vide" => "video",
        "soun" => "audio",
        "sbtl" | "text" => "subtitle",
        _ => "unknown",
    };

    // Extract language from mdhd box
    let language = extract_language_from_mdhd(mdia);

    // Look for minf box (Media Information)
    let minf = find_box(mdia, "minf")?;

    // Look for stbl box (Sample Table)
    let stbl = find_box(minf, "stbl")?;

    // Look for stsd box (Sample Description)
    let stsd = find_box(stbl, "stsd")?;

    // Extract codec and other details from stsd
    let (codec_id, frame_rate, width, height, channels) = extract_details_from_stsd(stsd, kind)?;

    Some(StreamInfo {
        index,
        kind: kind.to_string(),
        codec_id,
        frame_rate,
        width,
        height,
        channels,
        language,
    })
}

#[cfg(test)]
mod tests {
    use crate::mp4::r#box::write_box_header;
    use crate::mp4::trak::extract_stream_info_from_trak;

    fn make_box(name: &str, payload: &[u8]) -> Vec<u8> {
        let mut buf = Vec::new();
        write_box_header(&mut buf, name, (payload.len() + 8) as u32);
        buf.extend_from_slice(payload);
        buf
    }

    fn build_trak_box() -> Vec<u8> {
        let mut stsd_payload = vec![0, 0, 0, 0, 0, 0, 0, 1];
        stsd_payload.extend_from_slice(&[
            0x00, 0x00, 0x00, 0x24, b'a', b'v', b'c', b'1', 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x02, 0x80, 0x01, 0xE0, 0x00, 0x00, 0x00, 0x00,
        ]);
        let stsd_box = make_box("stsd", &stsd_payload);
        let stbl_box = make_box("stbl", &stsd_box);
        let minf_box = make_box("minf", &stbl_box);

        let mdhd_payload = [
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0x15, 0xc7, 0, 0,
        ];
        let mdhd_box = make_box("mdhd", &mdhd_payload);

        let hdlr_payload = [
            0, 0, 0, 0, 0, 0, 0, 0, b'v', b'i', b'd', b'e', 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        let hdlr_box = make_box("hdlr", &hdlr_payload);

        let mdia_box = make_box("mdia", &[mdhd_box, hdlr_box, minf_box].concat());
        make_box("trak", &mdia_box)
    }

    #[test]
    fn test_extract_stream_info_from_synthetic_trak() {
        let trak = build_trak_box();
        let info = extract_stream_info_from_trak(&trak[8..], 0).expect("info");
        assert_eq!(info.kind, "video");
        assert_eq!(info.codec_id, "H.264/AVC");
        assert_eq!(info.width, Some(640));
        assert_eq!(info.height, Some(480));
        assert_eq!(info.language, Some("English".to_string()));
    }
}
