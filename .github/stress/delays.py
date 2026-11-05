"""How long after a verse is spoken it is detected, from a stress run's debug.log and the voice
timing of its script (voice-timing.yml: when each word starts in the audio). Windows' voice reads
the script the same way every time, so the times line up with any run of it.
Usage: delays.py <timing.tsv> <script.txt> <debug.log> [minutes played]"""
import bisect
import json
import re
import statistics
import sys

timing_path, script_path, log_path = sys.argv[1:4]
played_ms = float(sys.argv[4] if len(sys.argv) > 4 else 180) * 60000

# Word starts, by character position in the text as the voice read it (CRLF line ends).
# Windows' voice writes 22,050 Hz audio but reports its positions as if it were 16,000 Hz: every
# time it gives is 22050/16000 too long (measured against the stress run, 2026-10-31).
SCALE = 16000 / 22050
words = sorted((int(p), round(int(ms) * SCALE)) for p, ms in (l.split('\t') for l in open(timing_path, encoding='utf-8-sig') if '\t' in l))
pos = [p for p, _ in words]
text = open(script_path, encoding='utf-8', newline='').read()

def spoken(char):
    """When the word at or after this character starts, in ms into the audio."""
    i = bisect.bisect_left(pos, char)
    return words[min(i, len(words) - 1)][1]

BOOK = {'Psalm': 'Psalms', 'First Corinthians': '1 Corinthians', 'First Peter': '1 Peter'}
REF = re.compile(r'^(?:Turn with me to|Open your Bibles to|Look at|The Bible says in) (.+?) chapter (\d+) verses? (\d+)(?: to (\d+))?\.$')

refs, at = [], 0
for line in text.split('\r\n'):
    m = REF.match(line)
    if m:
        book, c, v = BOOK.get(m[1], m[1]), int(m[2]), int(m[3])
        # From when the (first) verse number is said: the last number in a single reference.
        num = line.rfind(' ' + m[3] + (' to' if m[4] else '.')) + 1
        refs.append((f'{book} {c}:{v}', spoken(at + num)))
    at += len(line) + 2
refs = [r for r in refs if r[1] < played_ms]

log = [json.loads(l) for l in open(log_path, encoding='utf-8', errors='replace') if l.startswith('{')]
# Playback started when the first audio arrived: the first 3-s audio line with frames in, less
# the frames counted in it (1024 samples = 64 ms each).
first = next(l for l in log if l['event'] == 'server:audio' and l['data'].get('framesIn'))
t0 = first['t'] - first['data']['framesIn'] * 64
detects = [(l['t'], l['data']['target'], l['data']['verses']) for l in log if l['event'] == 'server:detect']

rows = []
for ref, ms in refs:
    said = t0 + ms
    hit = next(((t, target) for t, target, verses in detects if said - 2000 <= t <= said + 60000 and ref in verses), None)
    rows.append((ms, ref, None if hit is None else (hit[0] - said) / 1000, hit and hit[1]))

def line(label, rs):
    lags = [r[2] for r in rs if r[2] is not None]
    if not rs:
        return
    q = statistics.quantiles(lags, n=10) if len(lags) > 1 else lags * 9
    print(f'{label:>9} | {len(lags):>3} of {len(rs):<3} | {statistics.median(lags) if lags else 0:>7.1f} | {q[8] if lags else 0:>7.1f} | {max(lags, default=0):>7.1f} | '
          f'{sum(1 for r in rs if r[3] == "screen"):>6}')

print(f'{len(refs)} verse references spoken in {played_ms / 60000:.0f} min; playback began {t0}')
print('half hour | detected | median s | 90% within s | slowest s | on screen')
for b in range(int(played_ms // 1800000) + 1):
    line(str(b + 1), [r for r in rows if int(r[0] // 1800000) == b])
line('all', rows)
missed = [f'{r[1]} at {r[0] / 60000:.1f} min' for r in rows if r[2] is None]
print(f'missed ({len(missed)}):', ', '.join(missed[:40]))
