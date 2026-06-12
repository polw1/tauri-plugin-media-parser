import { invoke } from '@tauri-apps/api/core';
// ============================================================================
// Functions
// ============================================================================
/**
 * Extract metadata from a media file (local path or URL).
 *
 * Automatically detects if the source is a URL (http:// or https://) or a local file path.
 *
 * @param source - Absolute path to a local file or URL of a remote media file
 * @param options - Optional settings (headers are only used for URLs)
 * @returns Metadata containing duration, timescale, and tags (title, artist, etc.)
 *
 * @example
 * ```typescript
 * // Local file
 * const metadata = await getMetadata('/path/to/video.mp4');
 *
 * // Remote URL
 * const metadata = await getMetadata('https://example.com/video.mp4');
 *
 * // Remote URL with authentication
 * const metadata = await getMetadata('https://example.com/video.mp4', {
 *    headers: { 'Authorization': 'Bearer token123' }
 * });
 *
 * console.log(`Duration: ${metadata.duration / metadata.timescale} seconds`);
 *
 * // Find title
 * const title = metadata.values.find(m => m.name === 'Title');
 * if (title) {
 *    console.log(`Title: ${title.value}`);
 * }
 * ```
 */
export async function getMetadata(source, options) {
    return await invoke('plugin:media-parser|get_metadata', {
        source,
        headers: options?.headers,
    });
}
/**
 * Extract tracks from a media file (local path or URL).
 *
 * @param source - Absolute path to a local file or URL of a remote media file
 * @param options - Optional settings (headers are only used for URLs)
 * @returns Track information for video, audio, subtitle, and unknown tracks
 */
export async function getTracks(source, options) {
    return await invoke('plugin:media-parser|get_tracks', {
        source,
        headers: options?.headers,
    });
}
/**
 * Extract embedded cover artwork from a media file (local path or URL).
 *
 * @param source - Absolute path to a local file or URL of a remote media file
 * @param options - Optional settings (headers are only used for URLs)
 * @returns Cover artwork when present, otherwise null
 */
export async function getCover(source, options) {
    return await invoke('plugin:media-parser|get_cover', {
        source,
        headers: options?.headers,
    });
}
/**
 * Extract subtitle tracks and cues from a media file (local path or URL).
 *
 * @param source - Absolute path to a local file or URL of a remote media file
 * @param options - Optional track/language filters and URL headers
 * @returns Subtitle tracks with decoded cues
 */
export async function getSubtitles(source, options) {
    return await invoke('plugin:media-parser|get_subtitles', {
        source,
        trackId: options?.trackId,
        language: options?.language,
        headers: options?.headers,
    });
}
/**
 * Extract multiple thumbnails/frames for specific millisecond timestamps.
 *
 * Useful for building trimmer timelines. Missing/out-of-range timestamps are
 * ignored, so the returned array may be shorter than `timestamps`.
 *
 * @param source - Absolute path to a local file or URL of a remote media file
 * @param options - Timestamp range/list, optional track, accuracy, and URL headers
 * @returns Thumbnail/frame objects in timestamp order
 */
export async function getThumbnails(source, options) {
    const raw = await invoke('plugin:media-parser|get_thumbnails', {
        source,
        timestamps: options.timestamps,
        trackId: options.trackId,
        accurate: options.accurate,
        headers: options.headers,
    });
    return decodeThumbnailEnvelope(raw);
}
/**
 * Decodes the binary thumbnail envelope returned by the Rust side:
 * `[u32 LE header length][JSON header with per-frame metadata][image bytes]`.
 */
function decodeThumbnailEnvelope(raw) {
    const buf = raw instanceof Uint8Array ? raw : new Uint8Array(raw);
    const view = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
    const headerLen = view.getUint32(0, true);
    const headerBytes = buf.subarray(4, 4 + headerLen);
    const dataStart = 4 + headerLen;
    const entries = JSON.parse(new TextDecoder().decode(headerBytes));
    return entries.map(({ offset, length, ...info }) => ({
        ...info,
        data: buf.subarray(dataStart + offset, dataStart + offset + length),
    }));
}
// ============================================================================
// Utility Functions
// ============================================================================
/**
 * Calculate the duration in seconds from metadata.
 *
 * @param metadata - The metadata object
 * @returns Duration in seconds
 *
 * @example
 * ```typescript
 * const metadata = await getMetadata('/path/to/video.mp4');
 * const seconds = getDurationInSeconds(metadata);
 * console.log(`Video is ${seconds} seconds long`);
 * ```
 */
export function getDurationInSeconds(metadata) {
    if (metadata.timescale === 0) {
        return 0;
    }
    return metadata.duration / metadata.timescale;
}
/**
 * Get a metadata value by friendly name (case-insensitive).
 *
 * @param metadata - The metadata object
 * @param name - The friendly name to search for (e.g., "Title", "Artist", "Album")
 * @returns The value if found, undefined otherwise
 *
 * @example
 * ```typescript
 * const metadata = await getMetadata('/path/to/video.mp4');
 * const title = getMetadataValue(metadata, 'title');
 * const artist = getMetadataValue(metadata, 'artist');
 * ```
 */
export function getMetadataValue(metadata, name) {
    const lowerName = name.toLowerCase();
    const meta = metadata.values.find((m) => m.name.toLowerCase() === lowerName);
    return meta?.value;
}
