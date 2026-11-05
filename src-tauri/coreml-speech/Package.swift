// swift-tools-version:5.10
// The offline speech engine on Apple Silicon: Nemotron Speech Streaming as Core ML, on the
// Neural Engine (FluidAudio), as a program of its own that speaks the speech worker's protocol
// (kairo-server/src/offline.rs). Built by scripts/build-coreml-speech.sh into src-tauri/bin.
import PackageDescription

let package = Package(
    name: "kairo-coreml-speech",
    platforms: [.macOS(.v14)],
    dependencies: [
        // Pinned: a new FluidAudio is measured (replay --offline) before it's taken.
        .package(url: "https://github.com/FluidInference/FluidAudio.git", revision: "1b0ed50d686873128a4129b3510ce3914c849bab"),
    ],
    targets: [
        .executableTarget(
            name: "kairo-coreml-speech",
            dependencies: [.product(name: "FluidAudio", package: "FluidAudio")]
        ),
    ]
)
