use crate::mp4::stsc::SampleToChunkEntry;
use crate::mp4::stts::SttsEntry;

/// Struct to represent a thumbnail with timestamp
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ThumbnailData {
    pub base64: String,
    pub timestamp: f64,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug)]
pub struct VideoTrackInfo {
    pub timescale: u32,
    pub _duration: u64,
    pub sample_count: u32,
    pub chunk_offsets: Vec<u64>,
    pub sample_sizes: Vec<u32>,
    pub sample_to_chunk: Vec<SampleToChunkEntry>,
    pub stts_entries: Vec<SttsEntry>,         // Sample timing
    pub stss_entries: Vec<u32>,               // Sync samples (I-frames)
    pub avcc: Option<crate::mp4::AvccConfig>, // AVCC configuration if present
}

#[derive(Debug, Clone)]
pub(crate) struct SampleRange {
    pub offset: u64,
    pub size: u32,
    pub sample_index: u32,
    pub timestamp: f64,
}
