#!/usr/bin/env bash
# Builds the sermon-notes writer for Apple Silicon (src-tauri/notes-writer: the Mac's own on-device
# model through FoundationModels) into src-tauri/bin/kairo-notes-writer, which ships inside the app.
# It links the framework weakly, so it starts on any macOS 14+ and reports "needs macOS 26" there.
#
#   scripts/build-notes-writer.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SRC="$ROOT/src-tauri/notes-writer/main.swift"
OUT="$ROOT/src-tauri/bin"

if [[ -x "$OUT/kairo-notes-writer" && -z "${FORCE:-}" && "$OUT/kairo-notes-writer" -nt "$SRC" ]]; then
  echo "kairo-notes-writer already at $OUT (FORCE=1 to rebuild)"; exit 0
fi

mkdir -p "$OUT"
swiftc -O -target arm64-apple-macos14.0 \
  -Xlinker -weak_framework -Xlinker FoundationModels \
  "$SRC" -o "$OUT/kairo-notes-writer"
strip -x "$OUT/kairo-notes-writer" 2>/dev/null || true
echo "built $OUT/kairo-notes-writer"
