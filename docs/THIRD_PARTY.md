# Third-party software shipped with Kairo

## FFmpeg

Kairo carries a small build of [FFmpeg](https://ffmpeg.org) (LGPL 2.1 or later)
and runs it as a separate program, only to turn a video its web view can't play
(mkv, avi, wmv, flv, mpg, ts, 3gp, ProRes …) into a standard MP4 the first time
it's used. The original file is never touched; the converted copy is kept in
the app's `media-cache` folder.

It is also the encoder for **live streaming** (Settings → Streaming,
`src-tauri/src/stream.rs`): Kairo's graphics, the Director's camera
(AVFoundation on macOS, DirectShow on Windows) and the audio input, encoded once
to H.264 and AAC and sent over RTMP or RTMPS to every destination (the `flv` and
`tee` muxers; TLS by the operating system: Secure Transport on macOS, Schannel
on Windows). The Linux build has no LGPL H.264 encoder and no TLS, so streaming
there needs an FFmpeg with H.264 set with `KAIRO_FFMPEG`.

- **Version:** 9.0.2, built from the official source
  (`ffmpeg-9.0.2.tar.xz`, SHA-256 `8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e`),
  unmodified. Source: <https://ffmpeg.org/releases/>.
- **Builds (LGPL components only — no GPL, no x264):**
  - macOS: `scripts/build-ffmpeg-macos.sh` — universal; H.264 by macOS's
    VideoToolbox.
  - Windows: `scripts/build-ffmpeg-linux.sh windows` (mingw-w64 cross-build) —
    H.264 by Media Foundation, VP9 as the fallback.
  - Linux: `scripts/build-ffmpeg-linux.sh linux` — VP9 (no LGPL H.264 encoder).
  The exact configure line is printed by `ffmpeg -version`. CI builds all three
  (the `ffmpeg` job in `.github/workflows/build.yml`) and the app bundles the one
  for its OS in `bin/` beside the UI files.
- **libvpx 1.17.0** (BSD-3-Clause) is linked into the Windows and Linux builds
  for the VP9 encoder; source: <https://github.com/webmproject/libvpx>.
- A different `ffmpeg` can be used by setting `KAIRO_FFMPEG`; with none bundled
  the app uses one on the PATH. For streaming, when the bundled one cannot
  stream (an older build without RTMP), the app looks for one on the PATH that
  can, and uses its hardware H.264 encoder where there is one (libx264 only as a
  last resort, from an FFmpeg the user installed, never shipped).

## NDI® runtime

NDI® is a registered trademark of Vizrt NDI AB (<https://ndi.video>).

Kairo does **not** ship NDI's files. The first time an NDI output is turned
on and no NDI runtime is present, Kairo offers to fetch NDI's own free
redistributable from NDI (`https://ndi.link/NDIRedistV6Apple`,
`https://ndi.link/NDIRedistV6`), after the operator accepts NDI's licence
(`kairo-server/src/ndi_runtime.rs`):

- **macOS:** the package must be signed by NewTek (team `W8U66ET244`) and
  notarized by Apple, and `libndi.dylib` inside it signed by the same team;
  only that library and NDI's `libndi_licenses.txt` are kept, in the app's
  data folder (`ndi/`). Nothing is installed system-wide; no password.
- **Windows:** the installer must carry a valid NewTek/NDI Authenticode
  signature; it then runs silently (Windows asks for permission once).
- **Linux:** NDI publishes no runtime download; the user installs NDI.

The sender (`src-tauri/src/ndi.rs`) loads Kairo's fetched copy first, then any
installed NDI runtime. Help → About & credits carries NDI's trademark notice.
Because NDI is fetched from NDI rather than shipped, Kairo's own licence
(open source or not) is unaffected.

## Blackmagic Design DeckLink

Kairo is compatible with Blackmagic Design DeckLink. The DeckLink API headers
(`/Mac/Include`, `/Win/Include`, `/Linux/Include` of the Desktop Video SDK)
are distributed under Blackmagic's permissive header licence (kept intact in
each file). The DeckLink driver library itself is not bundled: it comes with
Blackmagic Desktop Video, which DeckLink owners already have installed.

## Meaning models

Bundled for meaning-based detection, fetched at pinned revisions and checked by
checksum (`src-tauri/crates/kairo-eval/src/bin/bundle-data.rs`):

- **bge-small-en-v1.5** by BAAI (MIT), as the int8 ONNX export
  [`Xenova/bge-small-en-v1.5`](https://huggingface.co/Xenova/bge-small-en-v1.5)
  — the sentence embedder, and the verse index built from it.
- **ms-marco-MiniLM-L-6-v2** (Apache 2.0), as the int8 ONNX export
  [`Xenova/ms-marco-MiniLM-L-6-v2`](https://huggingface.co/Xenova/ms-marco-MiniLM-L-6-v2)
  — the cross-encoder that rereads the shortlist.

## Sound models (`databases/vad`)

- **Silero VAD** (`silero_vad.onnx`, MIT, <https://github.com/snakers4/silero-vad>):
  tells speech from silence for the interpreter check (`kairo-server/src/interpretation.rs`).
- **YAMNet** (`yamnet.onnx`, Apache 2.0, Copyright 2022 Google LLC; AudioSet labels CC BY 4.0):
  tells singing from preaching (`kairo-server/src/music.rs`). It is Google's YAMNet v1 as converted
  to ONNX by <https://huggingface.co/audiomagic/yamnet-onnx> (`yamnet.onnx`, SHA-256
  `d3835ffbbd4a1bb3e777f0ca217b5007907f5171dd5d17c4236b95b2af8f908e`), with its weights stored
  as 8-bit integers (onnxruntime dynamic quantisation, 16.1 → 5.0 MB; shipped file SHA-256
  `c0ff18ff08a7c90a657832c0c868b31c75c1608d504c5c5ba2b68b3a34af312f`). On eight service recordings
  it switched singing on and off at the same moments as the original, give or take a few short spans.
