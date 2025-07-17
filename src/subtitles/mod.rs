mod analyzer;
mod extractor;
mod parser;
mod types;
mod utils;

pub use extractor::{extract_local_subtitle_entries, extract_remote_subtitle_entries};
pub use types::SubtitleEntry;

// Exports for testing
pub use parser::parse_subtitle_sample_data;
pub use utils::format_timestamp;
#[cfg(test)]
pub mod unit_test;
