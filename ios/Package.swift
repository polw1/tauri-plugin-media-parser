// swift-tools-version: 5.9
import PackageDescription

let package = Package(
  name: "tauri-plugin-media-parser",
  platforms: [.iOS(.v13)],
  products: [.library(name: "tauri-plugin-media-parser", type: .static, targets: ["tauri-plugin-media-parser"])],
  targets: [.target(name: "tauri-plugin-media-parser", path: "Sources")]
)
