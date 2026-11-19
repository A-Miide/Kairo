#!/usr/bin/env python3
"""Kairo Wide: a wide, heavy display face made from Montserrat (SIL Open Font License 1.1, no reserved font name).

Montserrat is cut at one weight, every outline and every advance is stretched sideways, and the kerning is
stretched with it. The face is renamed, as the licence requires of a modified font; the licence and copyright
lines stay in the file.

  python3 scripts/make-wide-font.py [--weight 800] [--stretch 1.3] [--out src/fonts/kairo-wide-800.woff2]

Needs fontTools and brotli (pip install fonttools brotli).
"""
import argparse, os
from fontTools.ttLib import TTFont
from fontTools.varLib import instancer

ap = argparse.ArgumentParser()
ap.add_argument('--src', default=os.path.join(os.path.dirname(__file__), '..', 'src', 'fonts', 'montserrat-100-900-latin.woff2'))
ap.add_argument('--weight', type=float, default=800)
ap.add_argument('--stretch', type=float, default=1.3)
ap.add_argument('--tracking', type=float, default=0, help='extra space after every letter, in 1/1000 em')
ap.add_argument('--family', default='Kairo Wide')
ap.add_argument('--out', default=os.path.join(os.path.dirname(__file__), '..', 'src', 'fonts', 'kairo-wide-800.woff2'))
a = ap.parse_args()

font = TTFont(a.src)
font = instancer.instantiateVariableFont(font, {'wght': a.weight})
sx = a.stretch

# outlines
glyf = font['glyf']
for name in font.getGlyphOrder():
    g = glyf[name]
    if g.isComposite():
        for c in g.components:
            c.x = round(c.x * sx)
    elif g.numberOfContours > 0:
        g.coordinates.scale((sx, 1))
        g.coordinates.toInt()
    if g.numberOfContours != 0:
        g.recalcBounds(glyf)

# advances and side bearings
hmtx = font['hmtx']
for name in font.getGlyphOrder():
    adv, lsb = hmtx[name]
    hmtx[name] = (round(adv * sx + (a.tracking if adv else 0)), round(lsb * sx))

# kerning and mark positions
def walk(o, seen):
    if id(o) in seen:
        return
    seen.add(id(o))
    cls = type(o).__name__
    if cls == 'ValueRecord':
        for k in ('XPlacement', 'XAdvance'):
            if getattr(o, k, None):
                setattr(o, k, round(getattr(o, k) * sx))
        return
    if cls == 'Anchor':
        o.XCoordinate = round(o.XCoordinate * sx)
        return
    if isinstance(o, (list, tuple)):
        for i in o: walk(i, seen)
    elif isinstance(o, dict):
        for i in o.values(): walk(i, seen)
    elif hasattr(o, '__dict__'):
        for i in o.__dict__.values(): walk(i, seen)
if 'GPOS' in font:
    walk(font['GPOS'].table.LookupList, set())
for t in ('HVAR', 'MVAR', 'avar', 'gvar', 'fvar', 'STAT'):
    if t in font:
        del font[t]

# names and weight/width classes
name = font['name']
full = f'{a.family} ExtraBold'
for rec in list(name.names):
    if rec.nameID in (1, 16): rec.string = a.family
    elif rec.nameID in (2, 17): rec.string = 'Regular' if rec.nameID == 2 else 'ExtraBold'
    elif rec.nameID == 4: rec.string = full
    elif rec.nameID == 6: rec.string = a.family.replace(' ', '') + '-ExtraBold'
    elif rec.nameID == 3: rec.string = f'{a.family.replace(" ", "")};ExtraBold;Kairo'
    elif rec.nameID == 5: rec.string = 'Version 1.0; derived from Montserrat 9.000'
    elif rec.nameID == 7: rec.string = ''
font['OS/2'].usWeightClass = int(a.weight)
font['OS/2'].usWidthClass = 7
font['head'].macStyle = 0
font['OS/2'].xAvgCharWidth = round(font['OS/2'].xAvgCharWidth * sx)

flavor = 'woff2' if a.out.endswith('.woff2') else None
font.flavor = flavor
font.save(a.out)
print('wrote', a.out, os.path.getsize(a.out), 'bytes')
