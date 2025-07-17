use super::analyzer::analyze_video_track;
use super::decoder::{extract_nalus_from_sample_bytes, generate_thumbnails_from_nalus};
use super::types::{SampleRange, ThumbnailData, VideoTrackInfo};
use crate::metadata::{ContainerFormat, detect_format};
use crate::mp4::{build_sample_timestamps, find_moov_box_efficiently};
use crate::seekable_http_stream::SeekableHttpStream;
use crate::seekable_stream::{LocalSeekableStream, SeekableStream};
use std::collections::HashMap;
use std::io::{self, SeekFrom};

/// Extract thumbnails efficiently by analyzing headers first, then downloading only specific frame data
pub fn extract_remote_thumbnails(
    url: String,
    count: usize,
    max_width: u32,
    max_height: u32,
) -> io::Result<Vec<ThumbnailData>> {
    let stream = SeekableHttpStream::new(url)?;
    extract_thumbnails(stream, count, max_width, max_height)
}

/// Open local file and extract thumbnails
pub fn extract_local_thumbnails<P: AsRef<std::path::Path>>(
    path: P,
    count: usize,
    max_width: u32,
    max_height: u32,
) -> io::Result<Vec<ThumbnailData>> {
    let stream = LocalSeekableStream::open(path)?;
    extract_thumbnails(stream, count, max_width, max_height)
}

/// Core thumbnail extraction using any seekable stream
pub fn extract_thumbnails<S: SeekableStream>(
    mut stream: S,
    count: usize,
    max_width: u32,
    max_height: u32,
) -> io::Result<Vec<ThumbnailData>> {
    println!("Thumbnail Extraction");

    // Step 0: Detect file format first
    match detect_format(&mut stream) {
        Ok(ContainerFormat::MP3) => {
            println!("MP3 file detected - no subtitle extraction needed");
            stream.print_stats();
            return Ok(Vec::new());
        }
        Ok(format) if format.is_mp4_family() => {
            println!(
                "{} format detected - proceeding with subtitle extraction",
                format.name()
            );
        }
        Ok(format) => {
            println!(
                "Unsupported format: {} - only MP4 family formats support subtitles",
                format.name()
            );
            stream.print_stats();
            return Ok(Vec::new());
        }
        Err(e) => {
            println!(
                "Format detection failed: {} - attempting MP4 extraction anyway",
                e
            );
        }
    }

    // 1: moov
    let moov_info = find_moov_box_efficiently(&mut stream)?;
    let (moov_pos, moov_size) = (moov_info.position, moov_info.size);
    stream.seek(SeekFrom::Start(moov_pos))?;
    let mut moov_buffer = vec![0u8; moov_size as usize];
    stream.read_exact(&mut moov_buffer)?;
    println!("Read moov box: {} bytes", moov_size);

    // 2: analyze
    let video_track_info = analyze_video_track(&moov_buffer[8..])?;
    println!(
        "Found video track: {} samples, timescale: {}",
        video_track_info.sample_count, video_track_info.timescale
    );

    // 3: target
    let target_samples = calculate_target_samples_internal(&video_track_info, count);
    println!(
        "Target samples for {} thumbnails: {:?}",
        count, target_samples
    );

    // 4: ranges
    let sample_ranges = find_sample_byte_ranges(&video_track_info, &target_samples)?;
    println!(
        "Sample byte ranges calculated: {} ranges",
        sample_ranges.len()
    );

    // 5: download
    let sample_data = download_sample_ranges(&mut stream, &sample_ranges)?;
    println!("Downloaded {} bytes of sample data", sample_data.len());

    // Extract parameter sets
    let parameter_sets = if let Some(avcc) = &video_track_info.avcc {
        println!("Using AVCC configuration for parameter sets");
        let mut map = HashMap::new();

        // Add SPS (type 7)
        for sps in &avcc.sps {
            map.insert(7u8, sps.clone());
            println!("  Added SPS: {} bytes", sps.len());
        }

        // Add PPS (type 8)
        for pps in &avcc.pps {
            map.insert(8u8, pps.clone());
            println!("  Added PPS: {} bytes", pps.len());
        }

        map
    } else {
        println!("No AVCC config found, extracting parameter sets from samples");
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
    println!(
        "Generated {} thumbnails using direct NALU approach",
        thumbnails.len()
    );

    // stats
    stream.print_stats();
    Ok(thumbnails)
}

/// Download specific byte ranges
fn download_sample_ranges<S: SeekableStream>(
    stream: &mut S,
    ranges: &[SampleRange],
) -> io::Result<Vec<u8>> {
    let mut all_data = Vec::new();

    for range in ranges {
        stream.seek(SeekFrom::Start(range.offset))?;
        let mut sample_data = vec![0u8; range.size as usize];
        stream.read_exact(&mut sample_data)?;
        all_data.extend(sample_data);

        println!(
            "  Downloaded sample {} ({} bytes) from offset {} at {:.2}s",
            range.sample_index, range.size, range.offset, range.timestamp
        );
    }

    Ok(all_data)
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
) -> io::Result<Vec<SampleRange>> {
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
fn calculate_sample_offset(track_info: &VideoTrackInfo, sample_number: u32) -> io::Result<u64> {
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

    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "Sample not found",
    ))
}

/// Extract parameter sets (SPS/PPS) from sample data
fn extract_parameter_sets_from_samples(
    sample_data: &[u8],
    sample_ranges: &[SampleRange],
) -> io::Result<HashMap<u8, Vec<u8>>> {
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
                    println!(
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
