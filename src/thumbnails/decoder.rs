use super::types::{SampleRange, ThumbnailData};
use crate::avc::{extract_nalus_from_bytestream_new, extract_nalus_from_sample};
use openh264::decoder::Decoder;
use openh264::formats::YUVSource;
use std::collections::HashMap;
use std::io;

/// Generate thumbnails directly from H.264 sample data without MP4 container reconstruction
pub(crate) fn generate_thumbnails_from_nalus(
    sample_data: &[u8],
    sample_ranges: &[SampleRange],
    parameter_sets: &HashMap<u8, Vec<u8>>,
    count: usize,
    max_width: u32,
    max_height: u32,
) -> io::Result<Vec<ThumbnailData>> {
    println!("Generating thumbnails directly from NALUs...");

    println!(
        "Parameter sets ready: SPS={}, PPS={}",
        parameter_sets.contains_key(&7),
        parameter_sets.contains_key(&8)
    );

    // Create OpenH264 decoder directly for better performance
    let mut decoder =
        Decoder::new().map_err(|e| io::Error::other(format!("Failed to create decoder: {}", e)))?;

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
            println!("Sample {} extends beyond available data", i);
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
                println!(
                    "Generated thumbnail {} at {:.2}s",
                    thumbnails.len() + 1,
                    range.timestamp
                );
                thumbnails.push(thumbnail);
            }
            Err(e) => {
                println!("Failed to generate thumbnail from sample {}: {}", i, e);
            }
        }
    }

    if thumbnails.is_empty() {
        return Err(io::Error::other("No thumbnails could be generated"));
    }

    println!("Generated {} thumbnails from NALUs", thumbnails.len());
    Ok(thumbnails)
}

/// Initialize decoder with parameter sets once for better performance
fn initialize_decoder_with_parameter_sets(
    decoder: &mut Decoder,
    parameter_sets: &HashMap<u8, Vec<u8>>,
) -> io::Result<()> {
    // Send SPS first
    if let Some(sps) = parameter_sets.get(&7) {
        let mut sps_data = vec![0, 0, 0, 1];
        sps_data.extend_from_slice(sps);
        decoder.decode(&sps_data).map_err(|e| {
            io::Error::other(format!("Failed to initialize decoder with SPS: {}", e))
        })?;
    }

    // Send PPS after
    if let Some(pps) = parameter_sets.get(&8) {
        let mut pps_data = vec![0, 0, 0, 1];
        pps_data.extend_from_slice(pps);
        decoder.decode(&pps_data).map_err(|e| {
            io::Error::other(format!("Failed to initialize decoder with PPS: {}", e))
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
) -> Result<ThumbnailData, Box<dyn std::error::Error>> {
    // Extract NALUs from this sample
    let nalus = extract_nalus_from_sample_bytes(sample_bytes);

    if nalus.is_empty() {
        return Err("No NALUs found in sample".into());
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
        return Err("No video frame NALUs found in sample".into());
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
                    .ok_or("Failed to create RgbImage from RGB data")?;

            // Resize if necessary
            let resized_image = if rgb_image.width() > max_width || rgb_image.height() > max_height
            {
                super::utils::resize_image(rgb_image, max_width, max_height)
            } else {
                rgb_image
            };

            // Convert to base64
            let base64 = super::utils::image_to_base64(&resized_image)
                .map_err(|e| format!("Base64 conversion failed: {}", e))?;

            Ok(ThumbnailData {
                base64,
                timestamp,
                width: resized_image.width(),
                height: resized_image.height(),
            })
        }
        Ok(None) => Err("Decoder returned no frame".into()),
        Err(e) => Err(format!("H.264 decoding failed: {}", e).into()),
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
