//! MP4 atom (box) parsing utilities.
//!
//! This module provides types and functions for navigating and parsing
//! MP4/QuickTime container format atoms (also called boxes).
//!
//! ## Module Structure
//!
//! ```text
//! atoms/
//! ├── mod.rs      # Re-exports
//! ├── read.rs     # read_box - THE SINGLE PRIMITIVE
//! ├── types.rs    # Mp4Box enum
//! ├── iter.rs     # Mp4BoxIter, iter_boxes, find_box
//! ├── nav.rs      # find_box_ref, Mp4Nav trait
//! ├── moov.rs     # find_and_read_moov_box
//! └── tags.rs     # tag_name, fourcc_to_key
//! ```

mod iter;
mod media;
mod moov;
mod nav;
mod read;
mod tags;
mod types;

// Re-export public items
pub use iter::{Mp4BoxIter, iter_boxes};
pub use media::{
   CompositionOffsetEntry, MediaHeader, ParsedTrak, SampleCompositionOffset, SampleDescription,
   SamplePresentationTiming, SampleSelection, SampleSizes, SampleTiming, StscEntry, TrackHeader,
   TrackKind, decode_language, duration_to_ticks, expand_sample_composition_offsets,
   expand_sample_durations, expand_sample_sizes, fourcc_string, nearest_sync_sample,
   parse_chunk_offsets, parse_cover_art, parse_ctts, parse_hdlr, parse_mdhd,
   parse_sample_presentation_timings, parse_sample_sizes, parse_sample_timings, parse_stsc,
   parse_stsd, parse_stss, parse_tkhd, parse_trak, read_sample_data, read_sample_range,
   sample_file_offset, sample_size, select_sample_by_time, stts_sample_count, ticks_to_duration,
};
pub use moov::{find_and_read_moov_box, find_moov_bounds};
pub use nav::{Mp4Nav, find_box_ref};
pub use read::{BoxRead, read_box};
pub use tags::{fourcc_to_key, tag_name};
pub use types::Mp4Box;
