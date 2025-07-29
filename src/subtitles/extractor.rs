use super::analyzer::analyze_subtitle_tracks;
use super::parser::parse_subtitle_sample_data;
use super::types::{SubtitleEntry, SubtitleSampleRange, SubtitleTrackInfo};
use super::utils::{get_samples_in_chunk, group_nearby_subtitle_ranges};
use crate::errors::MediaParserResult;
use crate::metadata::{detect_format, ContainerFormat};
use crate::mp4::{build_sample_timestamps, find_moov_box_efficiently};
use crate::seekable_stream::SeekableStream;
use log::info;
use std::io::SeekFrom;

/// Core smart subtitle extraction for any SeekableStream
pub async fn extract_subtitle_entries<S: SeekableStream>(
    mut stream: S,
) -> MediaParserResult<Vec<SubtitleEntry>> {
    info!("Subtitle Extraction...");

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
            info!(
                "Unsupported format: {} - only MP4 family formats support subtitles",
                format.name()
            );
            stream.print_stats();
            return Ok(Vec::new());
        }
        Err(e) => {
            info!(
                "Format detection failed: {} - attempting MP4 extraction anyway",
                e
            );
        }
    }

    // Step 1: Find and read moov box
    let moov_info = find_moov_box_efficiently(&mut stream).await?;
    let (moov_pos, moov_size) = (moov_info.position, moov_info.size);
    stream.seek(SeekFrom::Start(moov_pos)).await?;
    let mut moov_buffer = vec![0u8; moov_size as usize];
    stream.read(&mut moov_buffer).await?;
    info!("Read moov box: {} bytes", moov_size);

    // Step 2: Analyze subtitle tracks
    let subtitle_tracks = analyze_subtitle_tracks(&moov_buffer[8..])?;
    if subtitle_tracks.is_empty() {
        info!("No subtitle tracks found");
        stream.print_stats();
        return Ok(Vec::new());
    }
    info!("Found {} subtitle tracks", subtitle_tracks.len());

    // Step 3: Use first track for extraction
    let first_track = &subtitle_tracks[0];
    let entries = extract_subtitles_with_intelligent_downloading(&mut stream, first_track).await?;
    info!("Extracted {} subtitle entries", entries.len());
    stream.print_stats();
    Ok(entries)
}

/// Extract subtitles using intelligent downloading (similar to thumbnails approach)
async fn extract_subtitles_with_intelligent_downloading<S: SeekableStream>(
    stream: &mut S,
    track: &SubtitleTrackInfo,
) -> MediaParserResult<Vec<SubtitleEntry>> {
    // Calculate subtitle sample ranges for targeted downloading
    let sample_ranges = calculate_optimized_subtitle_ranges(track)?;
    info!("Calculated {} subtitle sample ranges", sample_ranges.len());

    if sample_ranges.is_empty() {
        info!("No subtitle sample ranges found");
        return Ok(Vec::new());
    }

    // Group nearby ranges to minimize HTTP requests (similar to thumbnails), check this with Matthew**
    let optimized_ranges = group_nearby_subtitle_ranges(&sample_ranges, 4 * 1024); // 4KB grouping threshold
    info!("Optimized to {} download ranges", optimized_ranges.len());

    // Download subtitle data in optimized chunks
    let mut subtitle_entries = Vec::new();

    for range_group in optimized_ranges {
        let start_offset = range_group.first().unwrap().offset;
        let end_offset =
            range_group.last().unwrap().offset + range_group.last().unwrap().size as u64;
        let total_size = end_offset - start_offset;

        // Download the chunk
        stream.seek(SeekFrom::Start(start_offset)).await?;
        let mut chunk_data = vec![0u8; total_size as usize];
        stream.read(&mut chunk_data).await?;

        // Extract subtitle entries from this chunk
        for range in range_group {
            let relative_offset = (range.offset - start_offset) as usize;
            let sample_end = relative_offset + range.size as usize;

            if sample_end <= chunk_data.len() {
                let sample_data = &chunk_data[relative_offset..sample_end];

                // Parse subtitle data based on codec type
                if let Ok(entries) =
                    parse_subtitle_sample_data(sample_data, range.timestamp, &track.codec_type)
                {
                    subtitle_entries.extend(entries);
                }
            }
        }
    }

    // Sort by timestamp string (SRT format timestamps sort lexicographically)
    subtitle_entries.sort_by(|a, b| a.start.cmp(&b.start));

    Ok(subtitle_entries)
}

/// Calculate optimized subtitle sample ranges
fn calculate_optimized_subtitle_ranges(
    track: &SubtitleTrackInfo,
) -> MediaParserResult<Vec<SubtitleSampleRange>> {
    let mut ranges = Vec::new();
    let mut sample_index = 0;

    // Calculate timestamps for samples using timing information
    //let sample_timestamps = calculate_sample_timestamps(track);
    let sample_timestamps = build_sample_timestamps(track.timescale, &track.stts_entries);

    // Map samples to chunks and calculate byte ranges
    for (chunk_idx, &chunk_offset) in track.chunk_offsets.iter().enumerate() {
        let chunk_num = (chunk_idx + 1) as u32;

        // Find how many samples are in this chunk
        let samples_in_chunk = get_samples_in_chunk(chunk_num, &track.sample_to_chunk);

        let mut chunk_byte_offset = 0u64;
        for _ in 0..samples_in_chunk {
            if sample_index < track.sample_sizes.len() {
                let sample_size = track.sample_sizes[sample_index];

                // Skip empty samples
                if sample_size > 0 {
                    let sample_offset = chunk_offset + chunk_byte_offset;
                    let timestamp = sample_timestamps.get(sample_index).copied().unwrap_or(0.0);

                    ranges.push(SubtitleSampleRange {
                        offset: sample_offset,
                        size: sample_size,
                        _sample_index: sample_index as u32,
                        timestamp,
                    });
                }

                chunk_byte_offset += sample_size as u64;
                sample_index += 1;
            }
        }
    }

    Ok(ranges)
}

/// Calculate sample timestamps from timing tables
#[allow(dead_code)]
fn calculate_sample_timestamps(track: &SubtitleTrackInfo) -> Vec<f64> {
    let mut timestamps = Vec::new();
    let mut time_offset = 0u64;

    for stts_entry in &track.stts_entries {
        for _ in 0..stts_entry.sample_count {
            let timestamp = time_offset as f64 / track.timescale as f64;
            timestamps.push(timestamp);
            time_offset += stts_entry.sample_delta as u64;
        }
    }

    timestamps
}
