use crate::mp4::stsc::SampleToChunkEntry;
use crate::mp4::stts::SttsEntry;
use serde::Serialize;

/// Subtitle entry compatible with FFmpeg format
#[derive(Serialize, Debug)]
pub struct SubtitleEntry {
    pub start: String,
    pub end: String,
    pub text: String,
}

#[derive(Debug)]
pub(crate) struct SubtitleTrackInfo {
    pub _track_id: u32,
    pub timescale: u32,
    pub chunk_offsets: Vec<u64>,
    pub sample_sizes: Vec<u32>,
    pub sample_to_chunk: Vec<SampleToChunkEntry>,
    pub stts_entries: Vec<SttsEntry>, // Sample timing
    pub codec_type: String,
}

#[derive(Debug, Clone)]
pub(crate) struct SubtitleSampleRange {
    pub offset: u64,
    pub size: u32,
    pub _sample_index: u32,
    pub timestamp: f64,
}
