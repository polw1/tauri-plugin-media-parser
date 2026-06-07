const { invoke } = window.__TAURI__.core;

export async function getMetadata(source, options) {
  return await invoke('plugin:media-parser|get_metadata', {
    source,
    headers: options?.headers,
  });
}

export async function getTracks(source, options) {
  return await invoke('plugin:media-parser|get_tracks', {
    source,
    headers: options?.headers,
  });
}

export async function getSubtitles(source, options) {
  return await invoke('plugin:media-parser|get_subtitles', {
    source,
    trackId: options?.trackId,
    language: options?.language,
    headers: options?.headers,
  });
}


export async function getThumbnails(source, options) {
  return await invoke('plugin:media-parser|get_thumbnails', {
    source,
    timestamps: options.timestamps,
    trackId: options?.trackId,
    accurate: options?.accurate,
    headers: options?.headers,
  });
}

export function getDurationInSeconds(metadata) {
  if (metadata.timescale === 0) {
    return 0;
  }
  return metadata.duration / metadata.timescale;
}

export function getMetadataValue(metadata, name) {
  const lowerName = name.toLowerCase();
  const meta = metadata.values.find((item) => item.name.toLowerCase() === lowerName);
  return meta?.value;
}
