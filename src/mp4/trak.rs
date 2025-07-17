use crate::metadata::StreamInfo;
use crate::mp4::r#box::find_box;
use crate::mp4::mdhd::extract_language_from_mdhd;
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
    //println!("Found handler type: {}", handler_type);
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
