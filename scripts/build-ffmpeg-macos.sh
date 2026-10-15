#!/usr/bin/env bash
# Builds the small ffmpeg that ships inside Kairo on macOS: a universal
# (arm64 + x86_64) binary at src-tauri/bin/ffmpeg, from the official source,
# with the LGPL parts only. Kairo runs it as a separate program to convert a
# video its web view can't play (mkv, avi, wmv, …) into a standard MP4; macOS's
# own hardware H.264 encoder (VideoToolbox) does the encoding, so there is no
# x264 and no GPL code in it.
#
#   scripts/build-ffmpeg-macos.sh            # universal
#   scripts/build-ffmpeg-macos.sh arm64      # one architecture
set -euo pipefail

VERSION="9.0.2"
SHA256="8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e"
URL="https://ffmpeg.org/releases/ffmpeg-${VERSION}.tar.xz"

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/src-tauri/bin"
WORK="${KAIRO_FFMPEG_WORK:-$ROOT/target/ffmpeg-build}"
if [[ $# -eq 0 ]]; then ARCHS=(arm64 x86_64); else ARCHS=("$@"); fi

if [[ -x "$OUT/ffmpeg" && -z "${FORCE:-}" ]]; then
  echo "ffmpeg already at $OUT/ffmpeg (FORCE=1 to rebuild)"; exit 0
fi

mkdir -p "$WORK" "$OUT"
TAR="$WORK/ffmpeg-${VERSION}.tar.xz"
if [[ ! -f "$TAR" ]]; then curl -fsSL "$URL" -o "$TAR"; fi
echo "$SHA256  $TAR" | shasum -a 256 -c - >/dev/null || { echo "ffmpeg source checksum mismatch" >&2; rm -f "$TAR"; exit 1; }

DEMUXERS="matroska,avi,asf,flv,mov,mpegts,mpegps,mpegvideo,ogg,mxf,h264,hevc,m4v,dv,wav,mp3,aac,flac,ac3,eac3"
DECODERS="h264,hevc,mpeg1video,mpeg2video,mpeg4,msmpeg4v1,msmpeg4v2,msmpeg4v3,wmv1,wmv2,wmv3,vc1,vp8,vp9,theora,prores,mjpeg,dvvideo,h263,h263p,flv,rawvideo,\
aac,aac_latm,mp3,mp3float,mp2,ac3,eac3,dca,wmav1,wmav2,wmapro,vorbis,opus,flac,alac,pcm_s16le,pcm_s16be,pcm_s24le,pcm_s24be,pcm_s32le,pcm_f32le,pcm_u8,pcm_alaw,pcm_mulaw,adpcm_ms,adpcm_ima_wav"
PARSERS="h264,hevc,mpeg4video,mpegvideo,vc1,vp8,vp9,aac,aac_latm,ac3,mpegaudio,flac,opus,vorbis,dca,h263"

for ARCH in "${ARCHS[@]}"; do
  SRC="$WORK/src-$ARCH"
  BLD="$WORK/build-$ARCH"
  rm -rf "$SRC" "$BLD"; mkdir -p "$SRC" "$BLD"
  tar -xJf "$TAR" -C "$SRC" --strip-components=1
  EXTRA=()
  # No assembler for x86 here: the Intel slice is a little slower, and a conversion is a one-off.
  [[ "$ARCH" == "x86_64" ]] && EXTRA+=(--disable-x86asm)
  [[ "$(uname -m)" != "$ARCH" ]] && EXTRA+=(--enable-cross-compile)
  (
    cd "$BLD"
    "$SRC/configure" \
      --arch="$ARCH" --cc="clang -arch $ARCH" \
      --extra-cflags="-mmacosx-version-min=12.0" --extra-ldflags="-mmacosx-version-min=12.0" \
      --disable-everything --disable-autodetect --disable-debug --disable-doc \
      --disable-ffplay --disable-ffprobe --disable-shared --enable-static \
      --enable-zlib --enable-videotoolbox --enable-avfoundation --enable-securetransport --enable-network \
      --enable-demuxer="$DEMUXERS,rawvideo" --enable-decoder="$DECODERS" --enable-parser="$PARSERS" \
      --enable-encoder=h264_videotoolbox,aac,rawvideo --enable-muxer=mp4,rawvideo,flv,tee \
      --enable-protocol=file,pipe,tcp,tls,rtmp,rtmps \
      --enable-indev=avfoundation,lavfi \
      --enable-filter=scale,format,null,anull,aresample,aformat,setsar,fps,crop,pad,overlay,streamselect,color,anullsrc \
      --enable-bsf=aac_adtstoasc,extract_extradata \
      ${EXTRA[@]+"${EXTRA[@]}"} >"$WORK/configure-$ARCH.log" 2>&1 || { tail -30 "$WORK/configure-$ARCH.log" >&2; exit 1; }
    make -j"$(sysctl -n hw.ncpu)" ffmpeg >"$WORK/make-$ARCH.log" 2>&1 || { tail -30 "$WORK/make-$ARCH.log" >&2; exit 1; }
  )
  strip -S -x "$BLD/ffmpeg" 2>/dev/null || true
done

if [[ ${#ARCHS[@]} -eq 2 ]]; then
  lipo -create "$WORK/build-arm64/ffmpeg" "$WORK/build-x86_64/ffmpeg" -output "$OUT/ffmpeg"
else
  cp "$WORK/build-${ARCHS[0]}/ffmpeg" "$OUT/ffmpeg"
fi
chmod +x "$OUT/ffmpeg"
# lipo drops the signature the linker adds; the app's own signing step re-signs it for a release.
codesign --force -s - "$OUT/ffmpeg" >/dev/null 2>&1 || true

# What this build is, for the licence notice.
"$OUT/ffmpeg" -hide_banner -version | head -3
echo "ffmpeg ${VERSION} (LGPL, built from source, VideoToolbox) -> $OUT/ffmpeg"
