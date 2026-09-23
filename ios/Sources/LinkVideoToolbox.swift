import VideoToolbox

@_cdecl("tauri_plugin_media_parser_link_videotoolbox")
public func linkVideoToolbox() {
  _ = VTIsHardwareDecodeSupported(kCMVideoCodecType_H264)
}
