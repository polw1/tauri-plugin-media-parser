use super::types::SubtitleSampleRange;
use crate::mp4::stsc::SampleToChunkEntry;

/// Format timestamp in SRT format
pub fn format_timestamp(seconds: f64) -> String {
    if seconds.is_nan() || seconds.is_infinite() || seconds < 0.0 {
        return "00:00:00,000".to_string();
    }

    let total_millis = (seconds * 1000.0) as u64;
    let millis = total_millis % 1000;
    let total_seconds = total_millis / 1000;
    let secs = total_seconds % 60;
    let total_minutes = total_seconds / 60;
    let minutes = total_minutes % 60;
    let hours = total_minutes / 60;

    format!("{:02}:{:02}:{:02},{:03}", hours, minutes, secs, millis)
}

/// Get the number of samples in a specific chunk
pub(crate) fn get_samples_in_chunk(chunk_num: u32, sample_to_chunk: &[SampleToChunkEntry]) -> u32 {
    for (i, entry) in sample_to_chunk.iter().enumerate() {
        if chunk_num >= entry.first_chunk {
            if i + 1 < sample_to_chunk.len() {
                if chunk_num < sample_to_chunk[i + 1].first_chunk {
                    return entry.samples_per_chunk;
                }
            } else {
                return entry.samples_per_chunk;
            }
        }
    }
    0
}

/// Group nearby subtitle ranges to minimize HTTP requests
pub(crate) fn group_nearby_subtitle_ranges(
    ranges: &[SubtitleSampleRange],
    max_gap: u64,
) -> Vec<Vec<SubtitleSampleRange>> {
    if ranges.is_empty() {
        return Vec::new();
    }

    let mut groups = Vec::new();
    let mut current_group = vec![ranges[0].clone()];

    for i in 1..ranges.len() {
        let prev_range = &ranges[i - 1];
        let curr_range = &ranges[i];

        let gap = curr_range.offset - (prev_range.offset + prev_range.size as u64);

        if gap <= max_gap {
            // Add to current group
            current_group.push(curr_range.clone());
        } else {
            // Start new group
            groups.push(current_group);
            current_group = vec![curr_range.clone()];
        }
    }

    // Add the last group
    groups.push(current_group);

    groups
}
