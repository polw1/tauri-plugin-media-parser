use super::r#box::find_box;

/// Parse stss (sync samples / I-frames) box (optional)
pub fn parse_stss_thumbnails(stbl: &[u8]) -> Option<Vec<u32>> {
    let stss = find_box(stbl, "stss")?;

    if stss.len() < 8 {
        return None;
    }

    let entry_count = u32::from_be_bytes([stss[4], stss[5], stss[6], stss[7]]);
    let mut sync_samples = Vec::new();

    for i in 0..entry_count {
        let entry_pos = 8 + (i * 4) as usize;
        if entry_pos + 4 <= stss.len() {
            let sample_number = u32::from_be_bytes([
                stss[entry_pos],
                stss[entry_pos + 1],
                stss[entry_pos + 2],
                stss[entry_pos + 3],
            ]);
            sync_samples.push(sample_number);
        }
    }

    Some(sync_samples)
}
