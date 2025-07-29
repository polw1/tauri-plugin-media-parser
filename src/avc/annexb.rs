use crate::avc::nalus::{
    extract_nalus_from_bytestream as extract_nalus_from_bytestream_new, extract_nalus_from_sample,
    Nalu,
};
use crate::NaluType;

/// Convert a bytestream with Annex B start codes to a sample using 4-byte lengths.
/// The conversion is performed in a new buffer which is returned.
pub fn convert_bytestream_to_nalu_sample(stream: &[u8]) -> Vec<u8> {
    let nalus = extract_nalus_from_bytestream_new(stream);
    let mut out = Vec::new();

    for nalu in nalus {
        let len = nalu.data.len() as u32;
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(&nalu.data);
    }

    out
}

/// Replace 4-byte lengths in a sample with start codes (Annex B).
pub fn convert_sample_to_bytestream(sample: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();

    if let Some(nalus) = extract_nalus_from_sample(sample) {
        for nalu in nalus {
            out.extend_from_slice(&[0, 0, 0, 1]);
            out.extend_from_slice(&nalu.data);
        }
    }

    out
}

/// Unified format converter that handles multiple conversion scenarios
pub struct FormatConverter;

impl FormatConverter {
    /// Convert between different AVC formats
    pub fn convert(data: &[u8], from_format: AvcFormat, to_format: AvcFormat) -> Vec<u8> {
        match (from_format, to_format) {
            (AvcFormat::AnnexB, AvcFormat::Sample) => convert_bytestream_to_nalu_sample(data),
            (AvcFormat::Sample, AvcFormat::AnnexB) => convert_sample_to_bytestream(data),
            (AvcFormat::AnnexB, AvcFormat::AnnexB) | (AvcFormat::Sample, AvcFormat::Sample) => {
                // No conversion needed, just return a copy
                data.to_vec()
            }
        }
    }

    /// Extract the first video NAL unit from any format
    pub fn extract_first_video_nalu(data: &[u8], format: AvcFormat) -> Option<Nalu> {
        let nalus = match format {
            AvcFormat::AnnexB => extract_nalus_from_bytestream_new(data),
            AvcFormat::Sample => extract_nalus_from_sample(data).unwrap_or_default(),
        };

        nalus.into_iter().find(|nalu| nalu.is_video())
    }

    /// Extract all NAL units of a specific type from any format
    pub fn extract_nalus_of_type(data: &[u8], format: AvcFormat, nalu_type: NaluType) -> Vec<Nalu> {
        let nalus = match format {
            AvcFormat::AnnexB => extract_nalus_from_bytestream_new(data),
            AvcFormat::Sample => extract_nalus_from_sample(data).unwrap_or_default(),
        };

        nalus
            .into_iter()
            .filter(|nalu| nalu.nalu_type == nalu_type)
            .collect()
    }
}

/// Represents different AVC data formats
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AvcFormat {
    /// Annex B format with start codes (0x00000001 or 0x000001)
    AnnexB,
    /// Sample format with 4-byte length prefixes
    Sample,
}

// Legacy functions for backward compatibility
/// Extract the first video NAL unit from a bytestream.
#[deprecated(
    since = "1.0.0",
    note = "Use FormatConverter::extract_first_video_nalu instead"
)]
/// Get first video nalu from bytestream function.
pub fn get_first_video_nalu_from_bytestream(stream: &[u8]) -> Option<Vec<u8>> {
    FormatConverter::extract_first_video_nalu(stream, AvcFormat::AnnexB).map(|nalu| nalu.data)
}

/// Extract all NAL units from a bytestream without their start codes.
#[deprecated(since = "1.0.0", note = "Use extract_nalus_from_bytestream instead")]
pub fn extract_nalus_from_bytestream(data: &[u8]) -> Vec<Vec<u8>> {
    extract_nalus_from_bytestream_new(data)
        .into_iter()
        .map(|nalu| nalu.data)
        .collect()
}

/// Extract all NAL units of a specific type from a bytestream.
/// If `stop_at_video` is true, scanning stops at the first video NAL unit.
#[deprecated(
    since = "1.0.0",
    note = "Use FormatConverter::extract_nalus_of_type instead"
)]
/// Extract nalus of type from bytestream function.
pub fn extract_nalus_of_type_from_bytestream(
    n_type: NaluType,
    data: &[u8],
    stop_at_video: bool,
) -> Vec<Vec<u8>> {
    let nalus = FormatConverter::extract_nalus_of_type(data, AvcFormat::AnnexB, n_type);

    if stop_at_video {
        // Stop at first video NALU
        let mut result = Vec::new();
        for nalu in nalus {
            let data = nalu.data.clone();
            result.push(data);
            if nalu.is_video() {
                break;
            }
        }
        result
    } else {
        nalus.into_iter().map(|nalu| nalu.data).collect()
    }
}
