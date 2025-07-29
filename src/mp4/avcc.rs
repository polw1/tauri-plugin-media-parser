//! A module for parsing AVCConfigurationBox (avcC) data.
//! Parses SPS and PPS NAL units for H.264 streams in AVCC format.

use crate::avc::nalus::extract_parameter_sets;
use crate::errors::{MediaParserError, MediaParserResult, Mp4Error};

/// Represents the parsed AVCDecoderConfigurationRecord (avcC) configuration.
#[derive(Debug, Clone)]
pub struct AvccConfig {
    /// configurationVersion
    pub configuration_version: u8,
    /// AVCProfileIndication
    pub profile: u8,
    /// profileCompatibility
    pub compatibility: u8,
    /// AVCLevelIndication
    pub level: u8,
    /// lengthSizeMinusOne
    pub length_size_minus_one: u8,
    /// Sequence Parameter Sets
    pub sps: Vec<Vec<u8>>,
    /// Picture Parameter Sets
    pub pps: Vec<Vec<u8>>,
}

impl AvccConfig {
    /// Parse AVCDecoderConfigurationRecord as defined in ISO/IEC 14496-15.
    ///
    /// data: full contents of the avcC box (excluding header).
    pub fn parse(data: &[u8]) -> MediaParserResult<Self> {
        let mut pos = 0;
        if data.len() < 7 {
            return Err(MediaParserError::Mp4(Mp4Error::Error {
                message: "avcC data too short".to_string(),
            }));
        }
        // configurationVersion
        let configuration_version = data[pos];
        pos += 1;
        // AVCProfileIndication
        let profile = data[pos];
        pos += 1;
        // profileCompatibility
        let compatibility = data[pos];
        pos += 1;
        // AVCLevelIndication
        let level = data[pos];
        pos += 1;
        // lengthSizeMinusOne: 6 bits reserved + 2 bits
        let length_size_minus_one = data[pos] & 0x03;
        pos += 1;
        // numOfSequenceParameterSets: 3 bits reserved + 5 bits count
        let num_sps = data[pos] & 0x1F;
        pos += 1;
        let mut sps = Vec::with_capacity(num_sps as usize);
        for _ in 0..num_sps {
            if pos + 2 > data.len() {
                return Err(MediaParserError::Mp4(Mp4Error::Error {
                    message: "Unexpected EOF while reading SPS length".to_string(),
                }));
            }
            let len = u16::from_be_bytes([data[pos], data[pos + 1]]) as usize;
            pos += 2;
            if pos + len > data.len() {
                return Err(MediaParserError::Mp4(Mp4Error::Error {
                    message: "Unexpected EOF while reading SPS data".to_string(),
                }));
            }
            sps.push(data[pos..pos + len].to_vec());
            pos += len;
        }
        // numOfPictureParameterSets
        if pos >= data.len() {
            return Err(MediaParserError::Mp4(Mp4Error::Error {
                message: "Unexpected EOF while reading PPS count".to_string(),
            }));
        }
        let num_pps = data[pos];
        pos += 1;
        let mut pps = Vec::with_capacity(num_pps as usize);
        for _ in 0..num_pps {
            if pos + 2 > data.len() {
                return Err(MediaParserError::Mp4(Mp4Error::Error {
                    message: "Unexpected EOF while reading PPS length".to_string(),
                }));
            }
            let len = u16::from_be_bytes([data[pos], data[pos + 1]]) as usize;
            pos += 2;
            if pos + len > data.len() {
                return Err(MediaParserError::Mp4(Mp4Error::Error {
                    message: "Unexpected EOF while reading PPS data".to_string(),
                }));
            }
            pps.push(data[pos..pos + len].to_vec());
            pos += len;
        }
        Ok(AvccConfig {
            configuration_version,
            profile,
            compatibility,
            level,
            length_size_minus_one,
            sps,
            pps,
        })
    }

    /// Extract parameter sets from any AVC data format
    /// Supports both AVCC format and raw NALU streams
    pub fn extract_parameter_sets_from_data(
        data: &[u8],
        is_avcc_format: bool,
    ) -> ParameterSetsResult {
        if is_avcc_format {
            // Parse as AVCC format
            let config = Self::parse(data)?;
            if config.sps.is_empty() {
                return Err(MediaParserError::Mp4(Mp4Error::Error {
                    message: "No SPS found in AVCC format".to_string(),
                }));
            }
            if config.pps.is_empty() {
                return Err(MediaParserError::Mp4(Mp4Error::Error {
                    message: "No PPS found in AVCC format".to_string(),
                }));
            }
            Ok((config.sps, config.pps))
        } else {
            // Extract from NALU stream
            let (sps_nalus, pps_nalus) = extract_parameter_sets(data, false);
            let sps = sps_nalus
                .into_iter()
                .map(|nalu| nalu.data)
                .collect::<Vec<_>>();
            let pps = pps_nalus
                .into_iter()
                .map(|nalu| nalu.data)
                .collect::<Vec<_>>();
            if sps.is_empty() {
                return Err(MediaParserError::Mp4(Mp4Error::Error {
                    message: "No SPS found in NALU stream".to_string(),
                }));
            }
            if pps.is_empty() {
                return Err(MediaParserError::Mp4(Mp4Error::Error {
                    message: "No PPS found in NALU stream".to_string(),
                }));
            }
            Ok((sps, pps))
        }
    }

    /// Get the first SPS for profile/level analysis
    pub fn get_first_sps(&self) -> Option<&[u8]> {
        self.sps.first().map(|sps| sps.as_slice())
    }

    /// Get the first PPS for analysis
    pub fn get_first_pps(&self) -> Option<&[u8]> {
        self.pps.first().map(|pps| pps.as_slice())
    }

    /// Check if configuration is valid
    pub fn is_valid(&self) -> bool {
        !self.sps.is_empty() && !self.pps.is_empty()
    }
}

/// Unified parameter set extractor that works with multiple formats
pub struct ParameterSetExtractor;

impl ParameterSetExtractor {
    /// Extract parameter sets from AVCC format
    pub fn from_avcc(data: &[u8]) -> ParameterSetsResult {
        AvccConfig::extract_parameter_sets_from_data(data, true)
    }

    /// Extract parameter sets from NALU stream
    pub fn from_nalu_stream(data: &[u8]) -> ParameterSetsResult {
        AvccConfig::extract_parameter_sets_from_data(data, false)
    }

    /// Extract parameter sets from sample format (4-byte lengths)
    pub fn from_sample(data: &[u8]) -> ParameterSetsResult {
        let (sps_nalus, pps_nalus) = extract_parameter_sets(data, true);
        let sps = sps_nalus
            .into_iter()
            .map(|nalu| nalu.data)
            .collect::<Vec<_>>();
        let pps = pps_nalus
            .into_iter()
            .map(|nalu| nalu.data)
            .collect::<Vec<_>>();
        if sps.is_empty() {
            return Err(MediaParserError::Mp4(Mp4Error::Error {
                message: "No SPS found in sample format".to_string(),
            }));
        }
        if pps.is_empty() {
            return Err(MediaParserError::Mp4(Mp4Error::Error {
                message: "No PPS found in sample format".to_string(),
            }));
        }
        Ok((sps, pps))
    }

    /// Auto-detect format and extract parameter sets
    pub fn auto_detect(data: &[u8]) -> ParameterSetsResult {
        // Try AVCC format first (has specific header structure)
        if data.len() >= 7 && data[0] == 1 {
            return Self::from_avcc(data);
        }

        // Try NALU stream format
        if data.len() >= 4 && (data[0..3] == [0, 0, 1] || data[0..4] == [0, 0, 0, 1]) {
            return Self::from_nalu_stream(data);
        }

        // Assume sample format as fallback
        Self::from_sample(data)
    }
}

/// Type alias for parameter set extraction results
pub type ParameterSetsResult = MediaParserResult<(Vec<Vec<u8>>, Vec<Vec<u8>>)>;
