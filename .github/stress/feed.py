"""Plays a recording into a running Kairo server at live pace, as the app streams a microphone,
with offline listening on, and reports how it held up: how far the speech engine fell behind,
audio dropped, CPU and memory of Kairo's processes, and what was detected, per half hour.
Usage: feed.py <16 kHz mono s16le .raw> <port> <app data dir>"""
import asyncio
import glob
import json
import os
import sys
import time
import urllib.request

import psutil
import websockets

raw, port, data_dir = sys.argv[1], int(sys.argv[2]), sys.argv[3]
FRAME = 1024 * 2  # 1024 samples, as the app sends them
FRAME_S = 1024 / 16000
SAMPLE_S = 15
BUCKET_S = 30 * 60

def post(path, body):
    req = urllib.request.Request(f'http://127.0.0.1:{port}{path}', data=json.dumps(body).encode(), headers={'Content-Type': 'application/json'})
    return json.loads(urllib.request.urlopen(req, timeout=600).read() or b'{}')

def kairo_procs():
    return [p for p in psutil.process_iter(['name']) if (p.info['name'] or '').lower().startswith('kairo')]

events = {'final': 0, 'detection': 0, 'speech-behind': 0, 'interpretation': 0}
samples = []  # (seconds into audio, cpu % of one core, MB)

async def listen():
    async with websockets.connect(f'ws://127.0.0.1:{port}/', max_size=None) as ws:
        async for m in ws:
            try:
                v = json.loads(m)
            except ValueError:
                continue
            t = v.get('type')
            if t == 'transcript' and v.get('isFinal'):
                events['interpretation' if v.get('interpretation') else 'final'] += 1
            elif t in ('detection', 'speech-behind'):
                events[t] += 1

async def sample(start):
    procs = {}
    while True:
        for p in kairo_procs():
            if p.pid not in procs:
                procs[p.pid] = p
                p.cpu_percent(None)
        await asyncio.sleep(SAMPLE_S)
        cpu = mem = 0.0
        for pid, p in list(procs.items()):
            try:
                cpu += p.cpu_percent(None)
                mem += p.memory_info().rss / 2**20
            except psutil.Error:
                del procs[pid]
        samples.append((time.time() - start, cpu, mem))

async def main():
    for _ in range(120):  # the detection engine loads first
        started = post('/api/start-listening', {'engine': 'offline'})
        if started.get('ok'):
            break
        await asyncio.sleep(5)
    print('start listening:', started, flush=True)
    if not started.get('ok'):
        sys.exit(1)
    listener = asyncio.create_task(listen())
    size = os.path.getsize(raw)
    start = time.time()
    sampler = asyncio.create_task(sample(start))
    async with websockets.connect(f'ws://127.0.0.1:{port}/?audio=1', max_size=None) as audio:
        with open(raw, 'rb') as f:
            n = 0
            while chunk := f.read(FRAME):
                wait = start + n * FRAME_S - time.time()
                if wait > 0:
                    await asyncio.sleep(wait)
                await audio.send(chunk)
                n += 1
                if n % int(600 / FRAME_S) == 0:
                    print(f'{n * FRAME_S / 60:.0f} of {size / FRAME * FRAME_S / 60:.0f} min played; {events}', flush=True)
    await asyncio.sleep(20)  # what was still being decoded
    post('/api/stop-listening', {})
    sampler.cancel()
    listener.cancel()
    report(n * FRAME_S)

def report(played):
    # debug.log: every 3 s, frames in from the app, frames the engine took, frames dropped.
    lines = []
    for path in sorted(glob.glob(os.path.join(data_dir, 'debug.log*')), reverse=True):
        lines += [json.loads(l) for l in open(path, encoding='utf-8', errors='replace') if l.startswith('{')]
    audio = [l for l in lines if l.get('event') == 'server:audio']
    behind = [l for l in lines if l.get('event') == 'server:speech-behind']
    t0 = audio[0]['t'] if audio else 0
    queued = worst = 0
    per = {}
    for l in audio:
        d = l['data']
        queued += d.get('framesIn', 0) - d.get('dropped', 0) - d.get('engineFrames', 0)
        b = int((l['t'] - t0) / 1000 // BUCKET_S)
        r = per.setdefault(b, {'dropped': 0, 'queue': 0, 'skipped': 0})
        r['dropped'] += d.get('dropped', 0)
        r['queue'] = max(r['queue'], queued)
        worst = max(worst, queued)
    for l in behind:
        per.setdefault(int((l['t'] - t0) / 1000 // BUCKET_S), {'dropped': 0, 'queue': 0, 'skipped': 0})['skipped'] += l['data'].get('skippedMs', 0)
    print(f'\n== {played / 3600:.2f} h played; {events}')
    print('half hour | CPU avg / peak (% of one core) | memory start / end (MB) | most behind (s) | audio dropped (s) | engine skipped (s)')
    for b in sorted(set(per) | {int(s[0] // BUCKET_S) for s in samples}):
        ss = [s for s in samples if int(s[0] // BUCKET_S) == b]
        r = per.get(b, {'dropped': 0, 'queue': 0, 'skipped': 0})
        cpu = f'{sum(s[1] for s in ss) / len(ss):.0f} / {max(s[1] for s in ss):.0f}' if ss else '-'
        mem = f'{ss[0][2]:.0f} / {ss[-1][2]:.0f}' if ss else '-'
        print(f'{b + 1:>9} | {cpu:>30} | {mem:>23} | {max(r["queue"], 0) * FRAME_S:>15.1f} | {r["dropped"] * FRAME_S:>17.1f} | {r["skipped"] / 1000:>18.1f}')
    print(f'most behind overall: {max(worst, 0) * FRAME_S:.1f} s; peak memory {max((s[2] for s in samples), default=0):.0f} MB')

asyncio.run(main())
