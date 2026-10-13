# Third-party software shipped with Kairo

## FFmpeg

Kairo carries a small build of [FFmpeg](https://ffmpeg.org) (LGPL 2.1 or later)
and runs it as a separate program, only to turn a video its web view can't play
(mkv, avi, wmv, flv, mpg, ts, 3gp, ProRes …) into a standard MP4 the first time
it's used. The original file is never touched; the converted copy is kept in
the app's `media-cache` folder.

- **Version:** 9.0.2, built from the official source
  (`ffmpeg-9.0.2.tar.xz`, SHA-256 `8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e`).
- **Build:** `scripts/build-ffmpeg-macos.sh` — LGPL components only (no GPL, no
  x264); H.264 is encoded by macOS's own VideoToolbox. The exact configure line
  is printed by `ffmpeg -version`.
- **Source:** the tarball above, from <https://ffmpeg.org/releases/>. The build
  is unmodified.
- **Where it lives:** `Contents/Resources/bin/ffmpeg` in the macOS app. Linux
  packages depend on the system `ffmpeg`. Windows builds don't carry one yet; the
  app uses an `ffmpeg` on the PATH if there is one.
