#!/usr/bin/env bash
# Builds the small ffmpeg that ships inside Kairo on Linux and Windows, from the
# official source, LGPL parts only: src-tauri/bin/ffmpeg (Linux) or ffmpeg.exe
# (Windows, cross-built with mingw-w64). Run on Ubuntu 24.04:
#
#   scripts/build-ffmpeg-linux.sh linux
#   scripts/build-ffmpeg-linux.sh windows
#
# Kairo runs it as a separate program to turn a video its web view can't play
# into a standard file. Windows encodes H.264 with Media Foundation; Linux has no
# LGPL H.264 encoder, so both can also write VP9 (libvpx, BSD) in an MP4 with AAC
# audio, which the Windows and Linux web views play.
set -euo pipefail

TARGET="${1:-linux}"
[[ "$TARGET" == linux || "$TARGET" == windows ]] || { echo "usage: $0 linux|windows" >&2; exit 2; }

FF_VERSION="9.0.2"
FF_SHA256="8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e"
FF_URL="https://ffmpeg.org/releases/ffmpeg-${FF_VERSION}.tar.xz"
VPX_VERSION="1.17.0"
VPX_SHA256="1020f184046187baa2985dbde38e0691f49c44088bca7a1842b0236c6081dc0a"
VPX_URL="https://github.com/webmproject/libvpx/archive/refs/tags/v${VPX_VERSION}.tar.gz"

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/src-tauri/bin"
WORK="${KAIRO_FFMPEG_WORK:-$ROOT/target/ffmpeg-build-$TARGET}"
PREFIX="$WORK/prefix"
mkdir -p "$WORK" "$OUT" "$PREFIX"

if [[ "$TARGET" == windows ]]; then BIN="ffmpeg.exe"; else BIN="ffmpeg"; fi
if [[ -f "$OUT/$BIN" && -z "${FORCE:-}" ]]; then echo "$OUT/$BIN already built (FORCE=1 to rebuild)"; exit 0; fi

fetch() { # url sha256 file
  [[ -f "$3" ]] || curl -fsSL "$1" -o "$3"
  echo "$2  $3" | sha256sum -c - >/dev/null || { echo "checksum mismatch: $3" >&2; rm -f "$3"; exit 1; }
}
fetch "$FF_URL" "$FF_SHA256" "$WORK/ffmpeg.tar.xz"
fetch "$VPX_URL" "$VPX_SHA256" "$WORK/libvpx.tar.gz"

CROSS=""; VPX_TARGET=""; FF_PLATFORM=()
if [[ "$TARGET" == windows ]]; then
  CROSS="x86_64-w64-mingw32-"
  VPX_TARGET="--target=x86_64-win64-gcc"
  # d3d11va: FFmpeg 9's Media Foundation encoder (mfenc.c) uses D3D11 types
  # unconditionally, and --disable-autodetect leaves it off.
  FF_PLATFORM=(--target-os=mingw32 --arch=x86_64 --cross-prefix="$CROSS" --enable-mediafoundation --enable-d3d11va --enable-encoder=h264_mf
               --enable-schannel --enable-protocol=tls,rtmps --enable-indev=dshow
               --extra-ldflags=-static)
fi

# ── libvpx (VP8/VP9, BSD) ──
rm -rf "$WORK/vpx-src" "$WORK/vpx-build"; mkdir -p "$WORK/vpx-src" "$WORK/vpx-build"
tar -xzf "$WORK/libvpx.tar.gz" -C "$WORK/vpx-src" --strip-components=1
( cd "$WORK/vpx-build"
  CROSS="$CROSS" "$WORK/vpx-src/configure" --prefix="$PREFIX" $VPX_TARGET \
    --disable-examples --disable-tools --disable-docs --disable-unit-tests --disable-install-bins \
    --enable-vp8 --enable-vp9 --enable-pic --enable-static --disable-shared >"$WORK/vpx-configure.log" 2>&1 \
    || { tail -30 "$WORK/vpx-configure.log" >&2; exit 1; }
  make -j"$(nproc)" >"$WORK/vpx-make.log" 2>&1 || { tail -30 "$WORK/vpx-make.log" >&2; exit 1; }
  make install >/dev/null 2>&1 )

# ── ffmpeg ──
DEMUXERS="matroska,avi,asf,flv,mov,mpegts,mpegps,mpegvideo,ogg,mxf,h264,hevc,m4v,dv,wav,mp3,aac,flac,ac3,eac3"
DECODERS="h264,hevc,mpeg1video,mpeg2video,mpeg4,msmpeg4v1,msmpeg4v2,msmpeg4v3,wmv1,wmv2,wmv3,vc1,vp8,vp9,theora,prores,mjpeg,dvvideo,h263,h263p,flv,rawvideo,\
aac,aac_latm,mp3,mp3float,mp2,ac3,eac3,dca,wmav1,wmav2,wmapro,vorbis,opus,flac,alac,pcm_s16le,pcm_s16be,pcm_s24le,pcm_s24be,pcm_s32le,pcm_f32le,pcm_u8,pcm_alaw,pcm_mulaw,adpcm_ms,adpcm_ima_wav"
PARSERS="h264,hevc,mpeg4video,mpegvideo,vc1,vp8,vp9,aac,aac_latm,ac3,mpegaudio,flac,opus,vorbis,dca,h263"

rm -rf "$WORK/ff-src" "$WORK/ff-build"; mkdir -p "$WORK/ff-src" "$WORK/ff-build"
tar -xJf "$WORK/ffmpeg.tar.xz" -C "$WORK/ff-src" --strip-components=1
( cd "$WORK/ff-build"
  export PKG_CONFIG_PATH="$PREFIX/lib/pkgconfig"
  "$WORK/ff-src/configure" \
    --disable-everything --disable-autodetect --enable-network --disable-debug --disable-doc \
    --disable-ffplay --disable-ffprobe --disable-shared --enable-static \
    --enable-zlib --enable-libvpx --pkg-config-flags=--static \
    --extra-cflags="-I$PREFIX/include" --extra-ldflags="-L$PREFIX/lib" \
    --enable-demuxer="$DEMUXERS,rawvideo" --enable-decoder="$DECODERS" --enable-parser="$PARSERS" \
    --enable-encoder=libvpx_vp9,aac,rawvideo --enable-muxer=mp4,rawvideo,flv,tee,fifo,mpegts \
    --enable-protocol=file,pipe,tcp,rtmp --enable-indev=lavfi \
    --enable-filter=scale,format,null,anull,aresample,aformat,setsar,fps,crop,pad,overlay,streamselect,color,anullsrc,split,amix,volume \
    --enable-bsf=aac_adtstoasc,extract_extradata,h264_mp4toannexb \
    "${FF_PLATFORM[@]+"${FF_PLATFORM[@]}"}" >"$WORK/ff-configure.log" 2>&1 \
    || { tail -40 "$WORK/ff-configure.log" >&2; exit 1; }
  make -j"$(nproc)" "$BIN" >"$WORK/ff-make.log" 2>&1 || { tail -40 "$WORK/ff-make.log" >&2; exit 1; } )

cp "$WORK/ff-build/$BIN" "$OUT/$BIN"
"${CROSS}strip" "$OUT/$BIN" 2>/dev/null || strip "$OUT/$BIN" 2>/dev/null || true
chmod +x "$OUT/$BIN"
ls -la "$OUT/$BIN"
if [[ "$TARGET" == linux ]]; then "$OUT/$BIN" -hide_banner -version | head -3; fi
echo "ffmpeg ${FF_VERSION} + libvpx ${VPX_VERSION} (LGPL/BSD, built from source, $TARGET) -> $OUT/$BIN"
