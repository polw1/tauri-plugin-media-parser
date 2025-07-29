pub mod annexb;
pub mod nalus;

pub mod avc_type;

// Export specific functions to avoid conflicts
pub use annexb::{convert_bytestream_to_nalu_sample, convert_sample_to_bytestream};
pub use avc_type::NaluType;
pub use nalus::{
    dump_nalu_types, extract_nalus_from_bytestream as extract_nalus_from_bytestream_new,
    extract_nalus_from_sample, extract_nalus_of_type, extract_parameter_sets,
};

// Re-export main types for convenience
pub use annexb::{AvcFormat, FormatConverter};
pub use nalus::Nalu;
