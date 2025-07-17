#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum NaluType {
    NonIDR = 1,
    IDR = 5,
    SEI = 6,
    SPS = 7,
    PPS = 8,
    AUD = 9,
    EOSeq = 10,
    EOStream = 11,
    Fill = 12,
    Other(u8),
}

impl std::fmt::Display for NaluType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            NaluType::NonIDR => "NonIDR_1",
            NaluType::IDR => "IDR_5",
            NaluType::SEI => "SEI_6",
            NaluType::SPS => "SPS_7",
            NaluType::PPS => "PPS_8",
            NaluType::AUD => "AUD_9",
            NaluType::EOSeq => "EndOfSequence_10",
            NaluType::EOStream => "EndOfStream_11",
            NaluType::Fill => "FILL_12",
            NaluType::Other(v) => return write!(f, "Other_{v}"),
        };
        f.write_str(s)
    }
}

impl NaluType {
    pub fn from_header_byte(b: u8) -> Self {
        match b & 0x1f {
            1 => NaluType::NonIDR,
            5 => NaluType::IDR,
            6 => NaluType::SEI,
            7 => NaluType::SPS,
            8 => NaluType::PPS,
            9 => NaluType::AUD,
            10 => NaluType::EOSeq,
            11 => NaluType::EOStream,
            12 => NaluType::Fill,
            v => NaluType::Other(v),
        }
    }

    pub fn is_video(&self) -> bool {
        matches!(self, NaluType::NonIDR | NaluType::IDR)
    }
}

pub fn find_nalu_types(sample: &[u8]) -> Vec<NaluType> {
    if sample.len() < 4 {
        return Vec::new();
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
        if pos >= sample.len() {
            break;
        }
        let ntype = NaluType::from_header_byte(sample[pos]);
        nalus.push(ntype);
        pos += len;
    }
    nalus
}

pub fn has_parameter_sets(sample: &[u8]) -> bool {
    let types = find_nalu_types_up_to_first_video(sample);
    let mut has_sps = false;
    let mut has_pps = false;
    for t in types {
        if t == NaluType::SPS {
            has_sps = true;
        }
        if t == NaluType::PPS {
            has_pps = true;
        }
        if has_sps && has_pps {
            return true;
        }
    }
    false
}

pub fn find_nalu_types_up_to_first_video(sample: &[u8]) -> Vec<NaluType> {
    if sample.len() < 4 {
        return Vec::new();
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
        if pos >= sample.len() {
            break;
        }
        let ntype = NaluType::from_header_byte(sample[pos]);
        nalus.push(ntype);
        pos += len;
        if ntype.is_video() {
            break;
        }
    }
    nalus
}

pub fn contains_nalu_type(sample: &[u8], ntype: NaluType) -> bool {
    let mut pos = 0usize;
    while pos + 4 <= sample.len() {
        let len = u32::from_be_bytes([
            sample[pos],
            sample[pos + 1],
            sample[pos + 2],
            sample[pos + 3],
        ]) as usize;
        pos += 4;
        if pos >= sample.len() {
            break;
        }
        if NaluType::from_header_byte(sample[pos]) == ntype {
            return true;
        }
        pos += len;
    }
    false
}

pub fn get_parameter_sets(sample: &[u8]) -> (Vec<Vec<u8>>, Vec<Vec<u8>>) {
    let mut sps = Vec::new();
    let mut pps = Vec::new();
    if sample.len() < 4 {
        return (sps, pps);
    }
    let mut pos = 0usize;
    while pos + 4 <= sample.len() {
        let len = u32::from_be_bytes([
            sample[pos],
            sample[pos + 1],
            sample[pos + 2],
            sample[pos + 3],
        ]) as usize;
        pos += 4;
        if pos >= sample.len() {
            break;
        }
        let ntype = NaluType::from_header_byte(sample[pos]);
        let end = std::cmp::min(pos + len, sample.len());
        match ntype {
            NaluType::SPS => sps.push(sample[pos..end].to_vec()),
            NaluType::PPS => pps.push(sample[pos..end].to_vec()),
            _ if ntype.is_video() => break,
            _ => {}
        }
        pos += len;
    }
    (sps, pps)
}

/// Return true if the sample contains an IDR NAL unit.
pub fn is_idr_sample(sample: &[u8]) -> bool {
    contains_nalu_type(sample, NaluType::IDR)
}

/// Check if a NAL unit type is a video slice.
pub fn is_video_nalu_type(ntype: NaluType) -> bool {
    matches!(ntype, NaluType::NonIDR | NaluType::IDR)
}
