use crate::NaluType;

/// Represents a NAL unit with its type and data
#[derive(Debug, Clone)]
pub struct Nalu {
    pub nalu_type: NaluType,
    pub data: Vec<u8>,
}

impl Nalu {
    /// Create a NALU from raw data
    pub fn new(data: Vec<u8>) -> Option<Self> {
        if data.is_empty() {
            return None;
        }
        let nalu_type = NaluType::from_header_byte(data[0]);
        Some(Nalu { nalu_type, data })
    }

    /// Check if this NALU is a video frame
    pub fn is_video(&self) -> bool {
        self.nalu_type.is_video()
    }

    /// Check if this NALU is a parameter set (SPS/PPS)
    pub fn is_parameter_set(&self) -> bool {
        matches!(self.nalu_type, NaluType::SPS | NaluType::PPS)
    }
}

/// Extract NAL units from a sample with 4-byte lengths.
/// Returns a vector of Nalu structs with type information.
/// If the sample is malformed, `None` is returned.
pub fn extract_nalus_from_sample(sample: &[u8]) -> Option<Vec<Nalu>> {
    if sample.len() < 4 {
        return None;
    }
    let mut pos = 0usize;
    let mut nalus = Vec::new();
    while pos + 4 <= sample.len() {
        let len = u32::from_be_bytes([
            sample[pos],
            sample[pos + 1],
            sample[pos + 2],
            sample[pos + 3],
        ]) as usize;
        pos += 4;
        if pos + len > sample.len() {
            return None;
        }
        if let Some(nalu) = Nalu::new(sample[pos..pos + len].to_vec()) {
            nalus.push(nalu);
        }
        pos += len;
    }
    Some(nalus)
}

/// Extract NAL units from a bytestream with Annex B start codes.
/// Returns a vector of Nalu structs with type information.
pub fn extract_nalus_from_bytestream(stream: &[u8]) -> Vec<Nalu> {
    let mut nalus = Vec::new();
    let mut pos = 0usize;
    let mut curr_start: Option<usize> = None;

    while pos + 3 <= stream.len() {
        if pos + 4 <= stream.len() && stream[pos..pos + 4] == [0, 0, 0, 1] {
            if let Some(s) = curr_start {
                let mut end = pos;
                while end > s && stream[end - 1] == 0 {
                    end -= 1;
                }
                if let Some(nalu) = Nalu::new(stream[s..end].to_vec()) {
                    nalus.push(nalu);
                }
            }
            curr_start = Some(pos + 4);
            pos += 4;
            continue;
        } else if stream[pos..pos + 3] == [0, 0, 1] {
            if let Some(s) = curr_start {
                let mut end = pos;
                while end > s && stream[end - 1] == 0 {
                    end -= 1;
                }
                if let Some(nalu) = Nalu::new(stream[s..end].to_vec()) {
                    nalus.push(nalu);
                }
            }
            curr_start = Some(pos + 3);
            pos += 3;
            continue;
        }
        pos += 1;
    }

    if let Some(s) = curr_start {
        let mut end = stream.len();
        while end > s && stream[end - 1] == 0 {
            end -= 1;
        }
        if let Some(nalu) = Nalu::new(stream[s..end].to_vec()) {
            nalus.push(nalu);
        }
    }
    nalus
}

/// Extract NAL units of a specific type from any format
pub fn extract_nalus_of_type(
    nalu_type: NaluType,
    data: &[u8],
    is_sample_format: bool,
) -> Vec<Nalu> {
    let all_nalus = if is_sample_format {
        extract_nalus_from_sample(data).unwrap_or_default()
    } else {
        extract_nalus_from_bytestream(data)
    };

    all_nalus
        .into_iter()
        .filter(|nalu| nalu.nalu_type == nalu_type)
        .collect()
}

/// Extract parameter sets (SPS/PPS) from any format
pub fn extract_parameter_sets(data: &[u8], is_sample_format: bool) -> (Vec<Nalu>, Vec<Nalu>) {
    let all_nalus = if is_sample_format {
        extract_nalus_from_sample(data).unwrap_or_default()
    } else {
        extract_nalus_from_bytestream(data)
    };

    let mut sps = Vec::new();
    let mut pps = Vec::new();

    for nalu in all_nalus {
        match nalu.nalu_type {
            NaluType::SPS => sps.push(nalu),
            NaluType::PPS => pps.push(nalu),
            _ => {}
        }
    }

    (sps, pps)
}

// Legacy functions for backward compatibility
/// Extract NAL units from a sample with 4-byte lengths.
/// Returns a vector of slices into the original sample.
/// If the sample is malformed, `None` is returned.
#[deprecated(since = "1.0.0", note = "Use extract_nalus_from_sample instead")]
pub fn get_nalus_from_sample(sample: &[u8]) -> Option<Vec<Vec<u8>>> {
    extract_nalus_from_sample(sample).map(|nalus| nalus.into_iter().map(|nalu| nalu.data).collect())
}

/// Display helper for NAL unit lists used in tests and debugging.
pub fn dump_nalu_types(sample: &[u8]) -> String {
    match extract_nalus_from_sample(sample) {
        Some(list) => list
            .iter()
            .map(|n| format!("{:?}", n.nalu_type))
            .collect::<Vec<_>>()
            .join(","),
        None => "<invalid>".to_string(),
    }
}
