#!/usr/bin/env bash
# Builds the offline speech engine for Apple Silicon (src-tauri/coreml-speech: Nemotron as Core ML
# on the Neural Engine) into src-tauri/bin/kairo-coreml-speech, which ships inside the app. Intel
# Macs, Windows and Linux use the ONNX engine built into kairo-server instead.
#
#   scripts/build-coreml-speech.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PKG="$ROOT/src-tauri/coreml-speech"
OUT="$ROOT/src-tauri/bin"

if [[ -x "$OUT/kairo-coreml-speech" && -z "${FORCE:-}" && "$OUT/kairo-coreml-speech" -nt "$PKG/Sources/kairo-coreml-speech/main.swift" && "$OUT/kairo-coreml-speech" -nt "$PKG/Package.swift" && -z "$(find "$PKG/patches" -newer "$OUT/kairo-coreml-speech")" ]]; then
  echo "kairo-coreml-speech already at $OUT (FORCE=1 to rebuild)"; exit 0
fi

# Kairo's changes to the pinned FluidAudio (patches/*.patch), applied to its checkout once.
swift package resolve --package-path "$PKG"
for patch in "$PKG"/patches/*.patch; do
  if ! git -C "$PKG/.build/checkouts/FluidAudio" apply --reverse --check "$patch" 2>/dev/null; then
    git -C "$PKG/.build/checkouts/FluidAudio" apply "$patch"
  fi
done

swift build -c release --package-path "$PKG" --arch arm64
BIN="$(swift build -c release --package-path "$PKG" --arch arm64 --show-bin-path)"
mkdir -p "$OUT"
cp "$BIN/kairo-coreml-speech" "$OUT/kairo-coreml-speech"
strip -x "$OUT/kairo-coreml-speech" 2>/dev/null || true
echo "built $OUT/kairo-coreml-speech"
