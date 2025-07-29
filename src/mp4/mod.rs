#[macro_use]
mod macros;
pub mod r#box;
pub use r#box::{find_box, find_box_range};
pub mod metadata_extractor; // New MP4-specific metadata extraction
pub mod moov;
pub mod moov_finder; // Unified moov box finding utilities
pub mod trak; // Debug utilities for MP4 analysis
pub use metadata_extractor::extract_mp4_metadata;
pub use moov_finder::{find_and_read_moov_box, find_moov_box_efficiently, MoovBoxInfo};
pub mod mdhd;
pub use mdhd::parse_mdhd;
pub mod stco;
pub use stco::{
    parse_stco_or_co64, parse_stco_or_co64_lenient, parse_stco_or_co64_subtitles,
    parse_stco_or_co64_thumbnails,
};
pub mod stsz;
pub use stsz::{parse_stsz, parse_stsz_lenient, parse_stsz_subtitles, parse_stsz_thumbnails};
pub mod stsc;
pub use stsc::{
    parse_stsc, parse_stsc_lenient, parse_stsc_subtitles, parse_stsc_thumbnails, SampleToChunkEntry,
};
pub mod stts;
pub use stts::{
    build_sample_timestamps, parse_stts, parse_stts_lenient, parse_stts_subtitles,
    parse_stts_thumbnails, SttsEntry,
};
pub mod stss;
pub use stss::parse_stss_thumbnails;
pub mod avcc;
pub use avcc::AvccConfig;
pub mod ftyp;
pub mod mvhd;
pub mod stsd;
pub mod udta;
