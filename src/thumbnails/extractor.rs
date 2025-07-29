use super::analyzer::analyze_video_track;
use super::decoder::{extract_nalus_from_sample_bytes, generate_thumbnails_from_nalus};
use super::types::{SampleRange, ThumbnailData, VideoTrackInfo};
use crate::errors::{MediaParserError, MediaParserResult, ThumbnailError};
use crate::metadata::{detect_format, ContainerFormat};
use crate::mp4::{build_sample_timestamps, find_moov_box_efficiently};
use crate::seekable_stream::SeekableStream;
use log::{debug, info, warn};
use std::collections::HashMap;
use std::io::{self, SeekFrom};

// Maximum allowed size for moov box to prevent OOM exceptions
// 50MB should handle most cases - typical 1080p hour-long videos: 5-20MB
// 4K movies (2+ hours) could be 50-200MB but we may need to make this adaptive in the future
const MAX_MOOV_SIZE: usize = 50 * 1024 * 1024; // 50MB limit

// Core thumbnail extraction using any seekable stream
pub async fn extract_thumbnails_generic<S: SeekableStream>(
    mut stream: S,
    count: usize,
    max_width: u32,
    max_height: u32,
) -> MediaParserResult<Vec<ThumbnailData>> {
    info!("Thumbnail Extraction");

    // Step 0: Detect file format first
    match detect_format(&mut stream).await {
        Ok(ContainerFormat::MP3) => {
            info!("MP3 file detected - no subtitle extraction needed");
            stream.print_stats();
            return Ok(Vec::new());
        }
        Ok(format) if format.is_mp4_family() => {
            info!(
                "{} format detected - proceeding with subtitle extraction",
                format.name()
            );
        }
        Ok(format) => {
            warn!(
                "Unsupported format: {} - only MP4 family formats support subtitles",
                format.name()
            );
            stream.print_stats();
            return Ok(Vec::new());
        }
        Err(e) => {
            warn!(
                "Format detection failed: {} - attempting MP4 extraction anyway",
                e
            );
        }
    }

    // 1: moov
    let moov_info = find_moov_box_efficiently(&mut stream).await?;
    let (moov_pos, moov_size) = (moov_info.position, moov_info.size);

    // Guard against extremely large moov boxes to prevent OOM
    if moov_size as usize > MAX_MOOV_SIZE {
        return Err(MediaParserError::Thumbnail(ThumbnailError::new(format!(
            "moov box too large: {} bytes (max allowed: {} bytes)",
            moov_size, MAX_MOOV_SIZE
        ))));
    }

    stream.seek(SeekFrom::Start(moov_pos)).await?;
    let mut moov_buffer = vec![0u8; moov_size as usize];
    stream.read(&mut moov_buffer).await?;
    info!("Read moov box: {} bytes", moov_size);

    // 2: analyze
    let video_track_info = analyze_video_track(&moov_buffer[8..])?;
    info!(
        "Found video track: {} samples, timescale: {}",
        video_track_info.sample_count, video_track_info.timescale
    );

    // 3: target
    let target_samples = calculate_target_samples_internal(&video_track_info, count);
    info!(
        "Target samples for {} thumbnails: {:?}",
        count, target_samples
    );

    // 4: ranges
    let sample_ranges = find_sample_byte_ranges(&video_track_info, &target_samples)?;
    info!(
        "Sample byte ranges calculated: {} ranges",
        sample_ranges.len()
    );

    // 5: download
    let sample_data = download_sample_ranges(&mut stream, &sample_ranges).await?;
    info!("Downloaded {} bytes of sample data", sample_data.len());

    // Extract parameter sets
    let parameter_sets = if let Some(avcc) = &video_track_info.avcc {
        info!("Using AVCC configuration for parameter sets");
        let mut map = HashMap::new();

        // Add SPS (type 7)
        for sps in &avcc.sps {
            map.insert(7u8, sps.clone());
            debug!("  Added SPS: {} bytes", sps.len());
        }

        // Add PPS (type 8)
        for pps in &avcc.pps {
            map.insert(8u8, pps.clone());
            debug!("  Added PPS: {} bytes", pps.len());
        }

        map
    } else {
        info!("No AVCC config found, extracting parameter sets from samples");
        extract_parameter_sets_from_samples(&sample_data, &sample_ranges)?
    };

    // 6: generate
    let thumbnails = generate_thumbnails_from_nalus(
        &sample_data,
        &sample_ranges,
        &parameter_sets,
        count,
        max_width,
        max_height,
    )?;
    info!(
        "Generated {} thumbnails using direct NALU approach",
        thumbnails.len()
    );

    // stats
    stream.print_stats();
    Ok(thumbnails)
}

/// Download specific byte ranges with batching optimization for adjacent ranges
async fn download_sample_ranges<S: SeekableStream>(
    stream: &mut S,
    ranges: &[SampleRange],
) -> io::Result<Vec<u8>> {
    let mut all_data = Vec::new();

    // Sort ranges by offset for optimal batching
    let mut sorted_ranges = ranges.to_vec();
    sorted_ranges.sort_by_key(|r| r.offset);

    // Merge adjacent ranges to reduce HTTP requests
    let merged_ranges = merge_adjacent_ranges(&sorted_ranges);

    debug!(
        "Downloading {} sample ranges (merged into {} batches)",
        ranges.len(),
        merged_ranges.len()
    );

    for (batch_idx, batch) in merged_ranges.iter().enumerate() {
        stream.seek(SeekFrom::Start(batch.start_offset)).await?;
        let mut batch_data = vec![0u8; batch.total_size as usize];
        let _ = stream.read(&mut batch_data).await;

        // Extract individual sample data from the batch
        for sample_range in &batch.sample_ranges {
            let sample_start = (sample_range.offset - batch.start_offset) as usize;
            let sample_end = sample_start + sample_range.size as usize;
            let sample_data = &batch_data[sample_start..sample_end];
            all_data.extend_from_slice(sample_data);

            debug!(
                "  Downloaded sample {} ({} bytes) from offset {} at {:.2}s (batch {})",
                sample_range.sample_index,
                sample_range.size,
                sample_range.offset,
                sample_range.timestamp,
                batch_idx
            );
        }
    }

    Ok(all_data)
}

/// Represents a batch of adjacent sample ranges that can be downloaded in a single request
#[derive(Debug)]
struct SampleRangeBatch {
    start_offset: u64,
    total_size: u64,
    sample_ranges: Vec<SampleRange>,
}

/// Merge adjacent sample ranges to optimize HTTP requests
fn merge_adjacent_ranges(ranges: &[SampleRange]) -> Vec<SampleRangeBatch> {
    if ranges.is_empty() {
        return Vec::new();
    }

    let mut batches = Vec::new();
    let mut current_batch = SampleRangeBatch {
        start_offset: ranges[0].offset,
        total_size: ranges[0].size as u64,
        sample_ranges: vec![ranges[0].clone()],
    };

    for range in &ranges[1..] {
        let current_end = current_batch.start_offset + current_batch.total_size;

        // Check if this range is adjacent to the current batch
        // We allow a small gap (up to 1KB) to still consider ranges as "adjacent"
        const MAX_GAP: u64 = 1024;
        if range.offset <= current_end + MAX_GAP {
            // Extend the current batch
            let new_end = range.offset + range.size as u64;
            let batch_end = current_batch.start_offset + current_batch.total_size;
            let additional_size = new_end.saturating_sub(batch_end);

            current_batch.total_size += additional_size;
            current_batch.sample_ranges.push(range.clone());
        } else {
            // Start a new batch
            batches.push(current_batch);
            current_batch = SampleRangeBatch {
                start_offset: range.offset,
                total_size: range.size as u64,
                sample_ranges: vec![range.clone()],
            };
        }
    }

    // Don't forget the last batch
    batches.push(current_batch);

    batches
}

/// Calculate which samples we need for thumbnails (prefer I-frames)
/// Calculate target sample indices for thumbnails
fn calculate_target_samples_internal(
    track_info: &VideoTrackInfo,
    thumbnail_count: usize,
) -> Vec<u32> {
    if !track_info.stss_entries.is_empty() {
        // Use I-frames if available
        let iframe_count = track_info.stss_entries.len();
        if iframe_count >= thumbnail_count {
            // Select evenly distributed I-frames
            let step = iframe_count / thumbnail_count;
            (0..thumbnail_count)
                .map(|i| track_info.stss_entries[i * step] - 1) // Convert to 0-based
                .collect()
        } else {
            // Use all I-frames if we don't have enough
            track_info.stss_entries.iter().map(|&s| s - 1).collect()
        }
    } else {
        // No I-frame info, distribute evenly across all samples
        let step = track_info.sample_count / thumbnail_count as u32;
        (0..thumbnail_count).map(|i| (i as u32) * step).collect()
    }
}

/// Find byte ranges for specific samples
fn find_sample_byte_ranges(
    track_info: &VideoTrackInfo,
    target_samples: &[u32],
) -> MediaParserResult<Vec<SampleRange>> {
    let mut ranges = Vec::new();

    // Calculate sample timestamps
    let sample_timestamps = build_sample_timestamps(track_info.timescale, &track_info.stts_entries);

    // For each target sample, find its byte range
    for &sample_num in target_samples {
        if sample_num < track_info.sample_count {
            let sample_offset = calculate_sample_offset(track_info, sample_num)?;
            let sample_size = track_info.sample_sizes[sample_num as usize];
            let timestamp = sample_timestamps
                .get(sample_num as usize)
                .copied()
                .unwrap_or(0.0);

            ranges.push(SampleRange {
                offset: sample_offset,
                size: sample_size,
                sample_index: sample_num,
                timestamp,
            });
        }
    }

    Ok(ranges)
}

/// Calculate the byte offset of a specific sample
fn calculate_sample_offset(
    track_info: &VideoTrackInfo,
    sample_number: u32,
) -> MediaParserResult<u64> {
    // Find which chunk contains this sample
    let mut current_sample = 0u32;
    let mut _chunk_index = 0usize;

    for (i, stsc_entry) in track_info.sample_to_chunk.iter().enumerate() {
        let next_first_chunk = track_info
            .sample_to_chunk
            .get(i + 1)
            .map(|e| e.first_chunk)
            .unwrap_or(track_info.chunk_offsets.len() as u32 + 1);

        let chunks_in_this_group = next_first_chunk - stsc_entry.first_chunk;
        let samples_in_this_group = chunks_in_this_group * stsc_entry.samples_per_chunk;

        if current_sample + samples_in_this_group > sample_number {
            // Sample is in this group
            let sample_in_group = sample_number - current_sample;
            _chunk_index = (stsc_entry.first_chunk - 1
                + sample_in_group / stsc_entry.samples_per_chunk)
                as usize;
            let sample_in_chunk = sample_in_group % stsc_entry.samples_per_chunk;

            // Calculate offset within chunk
            let chunk_offset = track_info.chunk_offsets[_chunk_index];
            let mut offset_in_chunk = 0u64;

            let first_sample_in_chunk = current_sample
                + (sample_in_group / stsc_entry.samples_per_chunk) * stsc_entry.samples_per_chunk;
            for s in first_sample_in_chunk..(first_sample_in_chunk + sample_in_chunk) {
                offset_in_chunk += track_info.sample_sizes[s as usize] as u64;
            }

            return Ok(chunk_offset + offset_in_chunk);
        }

        current_sample += samples_in_this_group;
    }

    Err(MediaParserError::Thumbnail(ThumbnailError::new(
        "Sample range calculation failed",
    )))
}

/// Extract parameter sets (SPS/PPS) from sample data
fn extract_parameter_sets_from_samples(
    sample_data: &[u8],
    sample_ranges: &[SampleRange],
) -> MediaParserResult<HashMap<u8, Vec<u8>>> {
    let mut parameter_sets = HashMap::new();
    let mut data_offset = 0;

    // Look through the first few samples to find parameter sets
    for (i, range) in sample_ranges.iter().enumerate() {
        if i >= 3 && parameter_sets.len() >= 2 {
            break; // Found enough parameter sets
        }

        let sample_size = range.size as usize;
        if data_offset + sample_size > sample_data.len() {
            break;
        }

        let sample_bytes = &sample_data[data_offset..data_offset + sample_size];
        data_offset += sample_size;

        // Try different NALU extraction methods
        let nalus = extract_nalus_from_sample_bytes(sample_bytes);

        for nalu in nalus {
            if !nalu.is_empty() {
                let nalu_type = nalu[0] & 0x1f;
                if nalu_type == 7 || nalu_type == 8 {
                    // SPS or PPS
                    parameter_sets.insert(nalu_type, nalu);
                    debug!(
                        "  Found {} in sample {}",
                        if nalu_type == 7 { "SPS" } else { "PPS" },
                        i
                    );
                }
            }
        }
    }

    Ok(parameter_sets)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::thumbnails::types::SampleRange;

    #[test]
    fn test_merge_adjacent_ranges() {
        // Create sample ranges that should be merged
        let ranges = vec![
            SampleRange {
                offset: 1000,
                size: 100,
                sample_index: 1,
                timestamp: 0.0,
            },
            SampleRange {
                offset: 1100, // Adjacent to first
                size: 150,
                sample_index: 2,
                timestamp: 1.0,
            },
            SampleRange {
                offset: 1300, // Small gap (50 bytes) - should still be merged
                size: 200,
                sample_index: 3,
                timestamp: 2.0,
            },
            SampleRange {
                offset: 3000, // Large gap (1500 bytes) - should start new batch
                size: 100,
                sample_index: 4,
                timestamp: 3.0,
            },
        ];

        let merged = merge_adjacent_ranges(&ranges);

        // Should have 2 batches: [1000-1500] and [3000-3100]
        assert_eq!(merged.len(), 2);

        // First batch should contain 3 samples
        assert_eq!(merged[0].sample_ranges.len(), 3);
        assert_eq!(merged[0].start_offset, 1000);
        // Total size should be from start (1000) to end of last sample (1300 + 200 = 1500)
        assert_eq!(merged[0].total_size, 500); // 1500 - 1000 = 500

        // Second batch should contain 1 sample
        assert_eq!(merged[1].sample_ranges.len(), 1);
        assert_eq!(merged[1].start_offset, 3000);
        assert_eq!(merged[1].total_size, 100);
    }
}
