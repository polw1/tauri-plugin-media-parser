import { invoke } from '@tauri-apps/api/core';

// ============================================================================
// Types
// ============================================================================

/**
 * Single extracted metadata item.
 */
export interface Meta {
   /** Raw metadata key (e.g., "@nam" for MP4, "TIT2" for MP3). */
   key: string;
   /** Friendly mapped name (e.g., "Title", "Artist", or "Unknown"). */
   name: string;
   /** Extracted value (UTF-8, trimmed of null padding). */
   value: string;
}

/**
 * Metadata extracted from a media file.
 */
export interface Metadata {
   /** Detected format name (e.g., "MP4/M4A/MOV", "MP3"). */
   format: string;
   /** All metadata items found. */
   values: Meta[];
   /** Time units per second for `duration`. */
   timescale: number;
   /** Total raw duration in `timescale` units. */
   duration: number;
}

/**
 * Options for metadata extraction.
 */
export interface MetadataOptions {
   /** Custom HTTP headers to send with the request (only used for URLs). */
   headers?: Record<string, string>;
}

/**
 * Track information extracted from a media file.
 */
export interface TrackInfo {
   kind: 'video' | 'audio' | 'subtitle' | 'unknown';
   id: number;
   codec: string;
   language?: string;
   timescale: number;
   duration: number;
   properties: Record<string, string>;
   width?: number;
   height?: number;
   channels?: number;
   sampleRate?: number;
}

/**
 * Subtitle cue with timestamps in seconds.
 */
export interface SubtitleCueInfo {
   cueId: number;
   startSec: number;
   endSec: number;
   text: string;
}

/**
 * Subtitle track and decoded cues.
 */
export interface SubtitleInfo {
   id: number;
   codec: string;
   language?: string;
   timescale: number;
   duration: number;
   cues: SubtitleCueInfo[];
}

/**
 * Options for subtitle extraction.
 */
export interface SubtitleOptions extends MetadataOptions {
   /** Optional track id filter. Use 0 to return the first subtitle track. */
   trackId?: number;
   /** Optional ISO-639 language filter. Ignored when `trackId` is set. */
   language?: string;
}

/**
 * Thumbnail/frame extracted from a media file.
 */
export interface ThumbnailInfo {
   trackId: number;
   width: number;
   height: number;
   timestampSec: number;
   format: string;
   mimeType: string;
   data: number[];
}

/**
 * Options for extracting a thumbnail strip.
 */
export interface ThumbnailsOptions extends MetadataOptions {
   /** Track id to extract from. Defaults to the first video track. */
   trackId?: number;
   /** Timestamps to extract, in milliseconds. Missing/out-of-range items are ignored. */
   timestamps: number[];
   /** Decode exact frames. Defaults to false for fast trimmer/keyframe thumbnails. */
   accurate?: boolean;
}

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
export async function getMetadata(
   source: string,
   options?: MetadataOptions,
): Promise<Metadata> {
   return await invoke<Metadata>('plugin:media-parser|get_metadata', {
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
export async function getTracks(
   source: string,
   options?: MetadataOptions,
): Promise<TrackInfo[]> {
   return await invoke<TrackInfo[]>('plugin:media-parser|get_tracks', {
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
export async function getSubtitles(
   source: string,
   options?: SubtitleOptions,
): Promise<SubtitleInfo[]> {
   return await invoke<SubtitleInfo[]>('plugin:media-parser|get_subtitles', {
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
export async function getThumbnails(
   source: string,
   options: ThumbnailsOptions,
): Promise<ThumbnailInfo[]> {
   return await invoke<ThumbnailInfo[]>('plugin:media-parser|get_thumbnails', {
      source,
      timestamps: options.timestamps,
      trackId: options.trackId,
      accurate: options.accurate,
      headers: options.headers,
   });
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
export function getDurationInSeconds(metadata: Metadata): number {
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
export function getMetadataValue(metadata: Metadata, name: string): string | undefined {
   const lowerName = name.toLowerCase();
   const meta = metadata.values.find((m) => m.name.toLowerCase() === lowerName);
   return meta?.value;
}
