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
   MediaHeader, SampleDescription, SampleSizes, SampleTiming, StscEntry, TrackHeader,
   decode_language, expand_sample_sizes, fourcc_string, parse_chunk_offsets, parse_hdlr,
   parse_mdhd, parse_sample_sizes, parse_sample_timings, parse_stsc, parse_stsd, parse_tkhd,
   read_sample_data, sample_file_offset, sample_size, stts_sample_count, ticks_to_duration,
};
pub use moov::find_and_read_moov_box;
pub use nav::{Mp4Nav, find_box_ref};
pub use read::{BoxRead, read_box};
pub use tags::{fourcc_to_key, tag_name};
pub use types::Mp4Box;
