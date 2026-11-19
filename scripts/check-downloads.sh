#!/usr/bin/env bash
# Every file the app downloads must be there before a release goes out.
#
#   scripts/check-downloads.sh
#
# Reads the addresses from the code itself: each kairo_download!("<tag>/<file>") (Kairo's own
# release files, see kairo-server/src/lib.rs) and each fallback in installer.rs, and asks GitHub
# for each one. Exits 1 when any is missing. Two releases went missing without anyone knowing
# (offline-model-multi-v1, bible-packs; review, 2026-10-18).
set -euo pipefail
cd "$(dirname "$0")/.."

base="https://github.com/A-Miide/Kairo/releases/download/"
# Allowed to be missing: the Bible list. Every language Kairo offers has its Bible inside the app,
# and the app falls back to that one when the list is not there (bible_packs.rs run_install).
optional="bible-packs/catalog.json"

urls=$(
  { grep -rhoE 'kairo_download!\("[^"]+"\)' src-tauri/crates src-tauri/src | sed -E 's/.*\("([^"]+)"\).*/\1/' | sed "s|^|$base|"
    grep -hoE 'fallback: "https://[^"]+"' src-tauri/crates/kairo-server/src/installer.rs | sed -E 's/fallback: "([^"]+)"/\1/'
  } | sort -u
)

missing=0
while read -r url; do
  [ -n "$url" ] || continue
  code=$(curl -sIL -o /dev/null -w '%{http_code}' --retry 2 --max-time 30 "$url" || echo 000)
  if [ "$code" = "200" ]; then
    echo "ok       $url"
  elif [ "${url#"$base"}" = "$optional" ]; then
    echo "optional $url ($code)"
  else
    echo "MISSING  $url ($code)"
    missing=1
  fi
done <<< "$urls"

exit $missing
