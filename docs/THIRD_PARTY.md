# Third-party software shipped with Kairo

## FFmpeg

Kairo carries a small build of [FFmpeg](https://ffmpeg.org) (LGPL 2.1 or later)
and runs it as a separate program, only to turn a video its web view can't play
(mkv, avi, wmv, flv, mpg, ts, 3gp, ProRes …) into a standard MP4 the first time
it's used. The original file is never touched; the converted copy is kept in
the app's `media-cache` folder.

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
  the app uses one on the PATH.
