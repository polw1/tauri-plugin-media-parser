use super::types::{SampleRange, ThumbnailData};
use crate::avc::{extract_nalus_from_bytestream_new, extract_nalus_from_sample};
use crate::errors::{MediaParserError, MediaParserResult, ThumbnailError};
use log::{info, warn};
use openh264::decoder::Decoder;
use openh264::formats::YUVSource;
use std::collections::HashMap;

/// Generate thumbnails directly from H.264 sample data without MP4 container reconstruction
pub(crate) fn generate_thumbnails_from_nalus(
    sample_data: &[u8],
    sample_ranges: &[SampleRange],
    parameter_sets: &HashMap<u8, Vec<u8>>,
    count: usize,
    max_width: u32,
    max_height: u32,
) -> MediaParserResult<Vec<ThumbnailData>> {
    info!("Generating thumbnails directly from NALUs...");

    info!(
        "Parameter sets ready: SPS={}, PPS={}",
        parameter_sets.contains_key(&7),
        parameter_sets.contains_key(&8)
    );

    // Create OpenH264 decoder directly for better performance
    let mut decoder = Decoder::new()
        .map_err(|e| ThumbnailError::new(format!("Failed to create decoder: {}", e)))?;

    // Initialize decoder with parameter sets once
    initialize_decoder_with_parameter_sets(&mut decoder, parameter_sets)?;

    let mut thumbnails = Vec::new();
    let mut data_offset = 0;

    // Process each sample range
    for (i, range) in sample_ranges.iter().enumerate() {
        if thumbnails.len() >= count {
            break;
        }

        let sample_size = range.size as usize;
        if data_offset + sample_size > sample_data.len() {
            warn!("Sample {} extends beyond available data", i);
            break;
        }

        let sample_bytes = &sample_data[data_offset..data_offset + sample_size];
        data_offset += sample_size;

        // Try to generate thumbnail from this sample
        match generate_optimized_thumbnail_from_sample(
            &mut decoder,
            sample_bytes,
            range.timestamp,
            max_width,
            max_height,
        ) {
            Ok(thumbnail) => {
                info!(
                    "Generated thumbnail {} at {:.2}s",
                    thumbnails.len() + 1,
                    range.timestamp
                );
                thumbnails.push(thumbnail);
            }
            Err(e) => {
                warn!("Failed to generate thumbnail from sample {}: {}", i, e);
            }
        }
    }

    if thumbnails.is_empty() {
        return Err(ThumbnailError::new("No thumbnails could be generated").into());
    }

    info!("Generated {} thumbnails from NALUs", thumbnails.len());
    Ok(thumbnails)
}

/// Initialize decoder with parameter sets once for better performance
fn initialize_decoder_with_parameter_sets(
    decoder: &mut Decoder,
    parameter_sets: &HashMap<u8, Vec<u8>>,
) -> MediaParserResult<()> {
    // Send SPS first
    if let Some(sps) = parameter_sets.get(&7) {
        let mut sps_data = vec![0, 0, 0, 1];
        sps_data.extend_from_slice(sps);
        decoder.decode(&sps_data).map_err(|e| {
            ThumbnailError::new(format!("Failed to initialize decoder with SPS: {}", e))
        })?;
    }

    // Send PPS after
    if let Some(pps) = parameter_sets.get(&8) {
        let mut pps_data = vec![0, 0, 0, 1];
        pps_data.extend_from_slice(pps);
        decoder.decode(&pps_data).map_err(|e| {
            ThumbnailError::new(format!("Failed to initialize decoder with PPS: {}", e))
        })?;
    }

    Ok(())
}

/// Generate thumbnail from sample using optimized OpenH264 decoder (no redundant parameter sets)
pub fn generate_optimized_thumbnail_from_sample(
    decoder: &mut Decoder,
    sample_bytes: &[u8],
    timestamp: f64,
    max_width: u32,
    max_height: u32,
) -> MediaParserResult<ThumbnailData> {
    // Extract NALUs from this sample
    let nalus = extract_nalus_from_sample_bytes(sample_bytes);

    if nalus.is_empty() {
        return Err(MediaParserError::Thumbnail(ThumbnailError::new(
            "No NALUs found in sample",
        )));
    }

    // Build frame data with only sample NALUs (parameter sets already initialized)
    let mut frame_data = Vec::new();

    // Add only video frame NALUs (skip parameter sets since decoder is already initialized)
    for nalu in &nalus {
        if !nalu.is_empty() {
            let nalu_type = nalu[0] & 0x1f;
            // Skip parameter sets (SPS=7, PPS=8) since they're already initialized
            if nalu_type != 7 && nalu_type != 8 {
                frame_data.extend_from_slice(&[0, 0, 0, 1]);
                frame_data.extend_from_slice(nalu);
            }
        }
    }

    if frame_data.is_empty() {
        return Err(MediaParserError::Thumbnail(ThumbnailError::new(
            "No video frame NALUs found in sample",
        )));
    }

    // Decode using OpenH264 directly
    match decoder.decode(&frame_data) {
        Ok(Some(yuv)) => {
            let dimensions = yuv.dimensions();
            let rgb_len = yuv.rgb8_len();
            let mut rgb_data = vec![0u8; rgb_len];
            yuv.write_rgb8(&mut rgb_data);

            // Convert RGB data to RgbImage efficiently
            let rgb_image =
                image::RgbImage::from_raw(dimensions.0 as u32, dimensions.1 as u32, rgb_data)
                    .ok_or_else(|| {
                        ThumbnailError::new("Failed to create RgbImage from RGB data")
                    })?;

            // Resize if necessary
            let resized_image = if rgb_image.width() > max_width || rgb_image.height() > max_height
            {
                super::utils::resize_image(rgb_image, max_width, max_height)
            } else {
                rgb_image
            };

            // Convert to base64
            let base64 = super::utils::image_to_base64(&resized_image)?;

            Ok(ThumbnailData {
                base64,
                timestamp,
                width: resized_image.width(),
                height: resized_image.height(),
            })
        }
        Ok(None) => Err(MediaParserError::Thumbnail(ThumbnailError::new(
            "Decoder returned no frame",
        ))),
        Err(e) => Err(MediaParserError::Thumbnail(ThumbnailError::new(format!(
            "H.264 decoding failed: {}",
            e
        )))),
    }
}

/// Extract NALUs from sample bytes using multiple methods
pub(crate) fn extract_nalus_from_sample_bytes(sample_bytes: &[u8]) -> Vec<Vec<u8>> {
    // Try method 1: Direct bytestream extraction
    let nalus = extract_nalus_from_bytestream_new(sample_bytes);
    if !nalus.is_empty() {
        return nalus.into_iter().map(|nalu| nalu.data).collect();
    }

    // Try method 2: Sample-specific extraction
    if let Some(nalus) = extract_nalus_from_sample(sample_bytes) {
        return nalus.into_iter().map(|nalu| nalu.data).collect();
    }

    // Try method 3: Look for NALU length prefixes (common in MP4 samples)
    extract_nalus_from_length_prefixed(sample_bytes)
}

/// Extract NALUs from length-prefixed format (common in MP4 samples)
fn extract_nalus_from_length_prefixed(data: &[u8]) -> Vec<Vec<u8>> {
    let mut nalus = Vec::new();
    let mut pos = 0;

    while pos + 4 <= data.len() {
        // Read 4-byte length prefix
        let length =
            u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        pos += 4;

        if pos + length <= data.len() {
            nalus.push(data[pos..pos + length].to_vec());
            pos += length;
        } else {
            break;
        }
    }

    nalus
}

#[cfg(test)]
mod test_helpers {
    use crate::thumbnails::types::SampleRange;

    /// Bytes extracted from `testdata/video.h264` containing the first two frames
    /// (SPS/PPS/AUD/SEI/IDR + one P-frame). This keeps the mock data small while
    /// still providing valid H.264 samples for the tests.
    pub const SAMPLE_DATA: [u8; 120] = [
        0x00, 0x00, 0x00, 0x01, 0x65, 0x88, 0x84, 0x00, 0x2b, 0xff, 0xfe, 0xf5, 0x27, 0xf8, 0x14,
        0xd5, 0x08, 0x44, 0x4b, 0xe1, 0x6b, 0x61, 0xed, 0xd4, 0xb7, 0x49, 0x30, 0xd1, 0x70, 0xb1,
        0x2d, 0xb3, 0xd0, 0x00, 0x00, 0x03, 0x00, 0x00, 0x03, 0x00, 0x00, 0x18, 0xee, 0xec, 0x61,
        0x1a, 0x66, 0xb1, 0x3e, 0x51, 0xb0, 0xa0, 0x00, 0x00, 0x03, 0x00, 0x5e, 0x40, 0x17, 0xe0,
        0x9a, 0x85, 0xa4, 0x3e, 0x43, 0xb0, 0x35, 0x43, 0xc0, 0x50, 0xc7, 0x58, 0xa7, 0x10, 0x02,
        0x04, 0x00, 0x00, 0x03, 0x00, 0x00, 0x03, 0x00, 0x00, 0x03, 0x00, 0x00, 0x03, 0x00, 0x00,
        0x03, 0x02, 0xdf, 0x00, 0x00, 0x00, 0x01, 0x09, 0xf0, 0x00, 0x00, 0x00, 0x01, 0x41, 0x9a,
        0x24, 0x6c, 0x42, 0xbf, 0xfd, 0xe1, 0x00, 0x00, 0x03, 0x00, 0x00, 0x03, 0x00, 0x6a, 0x40,
    ];

    pub const SPS_BYTES: [u8; 28] = [
        0x67, 0x4d, 0x40, 0x1e, 0xec, 0xc0, 0x50, 0x17, 0xfc, 0xb8, 0x0b, 0x50, 0x10, 0x10, 0x14,
        0x00, 0x00, 0x03, 0x01, 0xf4, 0x00, 0x00, 0x5d, 0xa8, 0x3c, 0x58, 0xb6, 0x68,
    ];

    pub const PPS_BYTES: [u8; 5] = [0x68, 0xe9, 0x79, 0xcb, 0x20];

    /// Mock sample data function.
    pub fn mock_sample_data() -> Vec<u8> {
        SAMPLE_DATA.to_vec()
    }

    /// Mock sample ranges function.
    pub fn mock_sample_ranges() -> Vec<SampleRange> {
        vec![
            SampleRange {
                offset: 0,
                size: 93,
                sample_index: 0,
                timestamp: 0.0,
            },
            SampleRange {
                offset: 93,
                size: 27,
                sample_index: 1,
                timestamp: 1.0 / 30.0,
            },
        ]
    }

    /// Mock sps function.
    pub fn mock_sps() -> Vec<u8> {
        SPS_BYTES.to_vec()
    }

    /// Mock pps function.
    pub fn mock_pps() -> Vec<u8> {
        PPS_BYTES.to_vec()
    }
}

#[test]
fn test_generate_thumbnails_with_multiple_samples() -> MediaParserResult<()> {
    use crate::thumbnails::decoder::generate_optimized_thumbnail_from_sample;
    use openh264::decoder::Decoder;
    use test_helpers::*;

    let mut decoder = Decoder::new().expect("Failed to create decoder");

    // SPS e PPS reais
    let sps = mock_sps();
    let pps = mock_pps();
    /// Função auxiliar para inicializar o decoder com SPS e PPS diretamente
    fn initialize_decoder_with_parameter_sets_simples(
        decoder: &mut Decoder,
        sps: &[u8],
        pps: &[u8],
    ) -> MediaParserResult<()> {
        // Enviar SPS
        let mut sps_data = vec![0, 0, 0, 1];
        sps_data.extend_from_slice(sps);
        decoder.decode(&sps_data).map_err(|e| {
            ThumbnailError::new(format!("Failed to initialize decoder with SPS: {}", e))
        })?;

        // Enviar PPS
        let mut pps_data = vec![0, 0, 0, 1];
        pps_data.extend_from_slice(pps);
        decoder.decode(&pps_data).map_err(|e| {
            ThumbnailError::new(format!("Failed to initialize decoder with PPS: {}", e))
        })?;

        Ok(())
    }

    initialize_decoder_with_parameter_sets_simples(&mut decoder, &sps, &pps)?;

    let sample_data = mock_sample_data();
    let sample_ranges = mock_sample_ranges();

    // Teste processando múltiplas amostras
    let mut thumbnails = Vec::new();

    for (i, range) in sample_ranges.iter().enumerate().take(3) {
        let sample_bytes = &sample_data[range.offset as usize..][..range.size as usize];

        match generate_optimized_thumbnail_from_sample(
            &mut decoder,
            sample_bytes,
            range.timestamp,
            320,
            180,
        ) {
            Ok(thumbnail) => {
                info!(
                    "✓ Sample {}: Generated thumbnail at {:.2}s: {}x{} | base64.len = {}",
                    i,
                    thumbnail.timestamp,
                    thumbnail.width,
                    thumbnail.height,
                    thumbnail.base64.len()
                );
                thumbnails.push(thumbnail);
            }
            Err(e) => {
                warn!("⚠ Sample {}: Failed to generate thumbnail: {}", i, e);
            }
        }
    }

    assert!(!thumbnails.is_empty());

    Ok(())
}

#[test]
fn test_thumbnail_resize_options() -> MediaParserResult<()> {
    use crate::thumbnails::decoder::generate_optimized_thumbnail_from_sample;
    use openh264::decoder::Decoder;
    use test_helpers::*;
    let mut decoder = Decoder::new().expect("Failed to create decoder");

    // Inicializar decoder com SPS/PPS
    let sps = mock_sps();
    let pps = mock_pps();
    fn initialize_decoder_with_parameter_sets_simples(
        decoder: &mut Decoder,
        sps: &[u8],
        pps: &[u8],
    ) -> MediaParserResult<()> {
        // Enviar SPS
        let mut sps_data = vec![0, 0, 0, 1];
        sps_data.extend_from_slice(sps);
        decoder.decode(&sps_data).map_err(|e| {
            ThumbnailError::new(format!("Failed to initialize decoder with SPS: {}", e))
        })?;

        // Enviar PPS
        let mut pps_data = vec![0, 0, 0, 1];
        pps_data.extend_from_slice(pps);
        decoder.decode(&pps_data).map_err(|e| {
            ThumbnailError::new(format!("Failed to initialize decoder with PPS: {}", e))
        })?;

        Ok(())
    }

    initialize_decoder_with_parameter_sets_simples(&mut decoder, &sps, &pps)?;

    let sample_data = mock_sample_data();
    let sample_ranges = mock_sample_ranges();
    let range = &sample_ranges[0];
    let sample_bytes = &sample_data[range.offset as usize..][..range.size as usize];

    // Testar diferentes tamanhos de thumbnail
    let sizes = [(160, 90), (320, 180), (640, 360)];

    for (width, height) in sizes {
        let thumbnail = generate_optimized_thumbnail_from_sample(
            &mut decoder,
            sample_bytes,
            range.timestamp,
            width,
            height,
        )
        .unwrap();

        info!(
            "✓ Size {}x{}: Generated thumbnail at {:.2}s | base64.len = {}",
            width,
            height,
            thumbnail.timestamp,
            thumbnail.base64.len()
        );

        // Verificar se as dimensões estão corretas ou proporcionais
        assert!(thumbnail.width <= width);
        assert!(thumbnail.height <= height);
    }

    Ok(())
}

#[test]
fn test_error_handling() -> MediaParserResult<()> {
    use crate::thumbnails::decoder::generate_optimized_thumbnail_from_sample;
    use openh264::decoder::Decoder;
    use test_helpers::*;

    let mut decoder = Decoder::new().expect("Failed to create decoder");

    // Inicializar decoder com SPS/PPS
    let sps = mock_sps();
    let pps = mock_pps();

    fn initialize_decoder_with_parameter_sets_simples(
        decoder: &mut Decoder,
        sps: &[u8],
        pps: &[u8],
    ) -> MediaParserResult<()> {
        // Enviar SPS
        let mut sps_data = vec![0, 0, 0, 1];
        sps_data.extend_from_slice(sps);
        decoder.decode(&sps_data).map_err(|e| {
            ThumbnailError::new(format!("Failed to initialize decoder with SPS: {}", e))
        })?;

        // Enviar PPS
        let mut pps_data = vec![0, 0, 0, 1];
        pps_data.extend_from_slice(pps);
        decoder.decode(&pps_data).map_err(|e| {
            ThumbnailError::new(format!("Failed to initialize decoder with PPS: {}", e))
        })?;

        Ok(())
    }
    initialize_decoder_with_parameter_sets_simples(&mut decoder, &sps, &pps)?;

    // Testar com dados inválidos
    let invalid_data = vec![0u8; 100]; // Dados aleatórios que não são NALUs válidos

    let result =
        generate_optimized_thumbnail_from_sample(&mut decoder, &invalid_data, 0.0, 320, 180);

    // Deveria falhar graciosamente
    assert!(result.is_err());
    info!("✓ Correctly handled invalid data: {:?}", result.err());

    Ok(())
}
