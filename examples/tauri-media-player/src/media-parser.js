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
  const raw = await invoke('plugin:media-parser|get_thumbnails', {
    source,
    timestamps: options.timestamps,
    trackId: options?.trackId,
    accurate: options?.accurate,
    headers: options?.headers,
  });

  return decodeThumbnailEnvelope(raw);
}

// Binary envelope: [u32 LE header length][JSON metadata header][image bytes]
function decodeThumbnailEnvelope(raw) {
  const buf = raw instanceof Uint8Array ? raw : new Uint8Array(raw);
  const view = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
  const headerLen = view.getUint32(0, true);
  const dataStart = 4 + headerLen;
  const entries = JSON.parse(new TextDecoder().decode(buf.subarray(4, dataStart)));

  return entries.map(({ offset, length, ...info }) => ({
    ...info,
    data: buf.subarray(dataStart + offset, dataStart + offset + length),
  }));
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
