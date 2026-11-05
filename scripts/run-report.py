#!/usr/bin/env python3
"""Reads debug.log and prints a run the way you'd read the sermon: each final
transcript line, with what was sent to the screen (+) or offered in Possible
Matches (?) right under it.

    python3 scripts/run-report.py [debug.log] [--from HH:MM:SS] [--to HH:MM:SS] [--sends-only]

Default log: databases/debug.log (the dev build). The installed app writes to
~/Library/Application Support/com.kairo.scripture/debug.log.
"""
import datetime
import json
import os
import sys

args = sys.argv[1:]
path = os.path.join(os.path.dirname(__file__), "..", "databases", "debug.log")
lo = hi = None
sends_only = False
i = 0
while i < len(args):
    a = args[i]
    if a == "--from":
        lo = args[i + 1]; i += 1
    elif a == "--to":
        hi = args[i + 1]; i += 1
    elif a == "--sends-only":
        sends_only = True
    else:
        path = a
    i += 1

rows = []
with open(path) as f:
    for line in f:
        try:
            rows.append(json.loads(line))
        except ValueError:
            pass  # a line cut by the log trimming


def clock(ms):
    return datetime.datetime.fromtimestamp(ms / 1000).strftime("%H:%M:%S")


shown = []
for r in sorted(rows, key=lambda r: r["t"]):
    if r["event"] not in ("server:transcript", "server:detect", "server:likely-wrong-send"):
        continue
    c = clock(r["t"])
    if (lo and c < lo) or (hi and c > hi):
        continue
    shown.append((c, r))

if not shown:
    sys.exit("No server:transcript or server:detect lines in that range. Run a build that logs them.")

for c, r in shown:
    d = r["data"]
    if r["event"] == "server:transcript":
        if not sends_only:
            print(f"{c}  {d['text']}")
    elif r["event"] == "server:likely-wrong-send":
        print(f"{c}      x BACK after {d['reference']} went up by itself  [{d['method']}, {d['secondsOnScreen']}s on screen]")
    else:
        mark = "+" if d["target"] == "screen" else "?"
        fix = f"  (corrects {d['correctsFrom']})" if d.get("correctsFrom") else ""
        print(f"{c}      {mark} {', '.join(d['verses'])}  [{d['method']} {d['score']:.2f}]{fix}")
