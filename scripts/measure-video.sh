#!/bin/sh
# Samples CPU and memory while Kairo runs, every 2 s.
#   app / server / speech : processes whose working directory is under $KAIRO_DIR
#                           (default: the folder this script lives in), so a
#                           second Kairo copy running beside it is not counted.
#   webkit                : every WebKit WebContent / GPU / Networking helper on
#                           the machine. Kairo's webviews run in these and macOS
#                           does not say which app owns which, so quit Safari and
#                           other WebKit apps (and any other Kairo copy) first.
#                           KAIRO_HELPERS=<regex> counts other helper processes instead (for a
#                           Chromium-based preview pane, e.g. 'Helper \\((Renderer|GPU)\\)').
# Usage: scripts/measure-video.sh [seconds=30] [label]
DIR=${KAIRO_DIR:-$(cd "$(dirname "$0")/.." && pwd)}
SECS=${1:-30}
LABEL=${2:-run}
N=$((SECS / 2))
OUT=$(mktemp)
i=0
while [ $i -lt $N ]; do
  MINE=$(lsof -d cwd -Fpn 2>/dev/null | awk -v d="$DIR" '/^p/{p=substr($0,2)} /^n/ && index($0,d)==2 {print p}' | tr '\n' ',' | sed 's/,$//')
  [ -n "$MINE" ] && ps -p "$MINE" -o pcpu=,rss=,command= | awk '
    /speech-worker/ { cls="speech" ; print cls, $1, $2; next }
    /kairo-server/  { cls="server" ; print cls, $1, $2; next }
    /Kairo/         { cls="app"    ; print cls, $1, $2; next }' >> "$OUT"
  ps -Ao pcpu=,rss=,command= | awk -v pat="${KAIRO_HELPERS:-com.apple.WebKit.(WebContent|GPU|Networking)}" '$0 ~ pat && !/awk/ { print "webkit", $1, $2 }' >> "$OUT"
  sleep 2
  i=$((i + 1))
done
echo "== $LABEL (${SECS}s) =="
awk '{ cpu[$1]+=$2; mem[$1]+=$3; seen[$1]=1 } END { for (c in seen) printf "%-8s cpu %6.1f%%  rss %6.0f MB\n", c, cpu[c]/'"$N"', mem[c]/'"$N"'/1024 }' "$OUT" | sort
awk '{ cpu+=$2; mem+=$3 } END { printf "%-8s cpu %6.1f%%  rss %6.0f MB\n", "TOTAL", cpu/'"$N"', mem/'"$N"'/1024 }' "$OUT"
rm -f "$OUT"
