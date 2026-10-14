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
