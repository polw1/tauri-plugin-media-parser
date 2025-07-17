/// Extract duration from mvhd box
pub fn extract_duration_from_mvhd(mvhd: &[u8]) -> Option<f64> {
    if mvhd.len() < 32 {
        return None;
    }

    let version = mvhd[0];
    if version == 0 {
        // Version 0: 32-bit values
        let timescale = u32::from_be_bytes([mvhd[12], mvhd[13], mvhd[14], mvhd[15]]);
        let duration = u32::from_be_bytes([mvhd[16], mvhd[17], mvhd[18], mvhd[19]]);

        if timescale > 0 {
            Some(duration as f64 / timescale as f64)
        } else {
            None
        }
    } else if version == 1 {
        // Version 1: 64-bit values
        if mvhd.len() < 44 {
            return None;
        }
        let timescale = u32::from_be_bytes([mvhd[20], mvhd[21], mvhd[22], mvhd[23]]);
        let duration = u64::from_be_bytes([
            mvhd[24], mvhd[25], mvhd[26], mvhd[27], mvhd[28], mvhd[29], mvhd[30], mvhd[31],
        ]);

        if timescale > 0 {
            Some(duration as f64 / timescale as f64)
        } else {
            None
        }
    } else {
        None
    }
}
