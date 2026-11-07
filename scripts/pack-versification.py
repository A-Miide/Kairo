#!/usr/bin/env python3
"""Renumbers the French pack (Louis Segond 1910, Hebrew verse numbering) to the KJV's.

Kairo keys every verse by its KJV reference, both to show a pack's text and to match speech against it.
Louis Segond counts a Psalm's title as verse 1 (or 1-2) and breaks some chapters where the Hebrew does
(Exodus 7/8, Leviticus 5/6, Joel-style splits in Hosea, Jonah, Micah, Nahum…), so its "Psalm 38:5" is the
KJV's 38:4. This moves each verse to its KJV number: titles are dropped, verses the KJV joins are joined.

    python3 scripts/pack-versification.py databases/bibles/packs/fr.json

Idempotent: a pack already in KJV numbering (verse counts equal to the KJV's) is left as it is.
"""
import json
import sys
from collections import OrderedDict

# Psalms whose title takes verse 1 (or verses 1-2) in the Hebrew numbering.
PSALM_TITLE_VERSES = {
    **{p: 1 for p in [3, 4, 5, 6, 7, 8, 9, 12, 18, 19, 20, 21, 22, 30, 31, 34, 36, 38, 39, 40, 41, 42, 44, 45, 46, 47, 48, 49,
                      53, 55, 56, 57, 58, 59, 61, 62, 63, 64, 65, 67, 68, 69, 70, 75, 76, 77, 80, 81, 83, 84, 85, 88, 89, 92,
                      102, 108, 140, 142]},
    **{p: 2 for p in [51, 52, 54, 60]},
}

# (book, chapter, first verse, last verse) in the pack → (chapter, first verse) in the KJV; a run of verses moves together.
RUNS = [
    ("Exodus", 7, 26, 29, 8, 1), ("Exodus", 8, 1, 28, 8, 5),
    ("Leviticus", 5, 20, 26, 6, 1), ("Leviticus", 6, 1, 23, 6, 8),
    ("Numbers", 30, 1, 1, 29, 40), ("Numbers", 30, 2, 17, 30, 1),
    ("1 Samuel", 24, 1, 1, 23, 29), ("1 Samuel", 24, 2, 23, 24, 1),
    ("1 Kings", 22, 45, 54, 22, 44),
    ("2 Chronicles", 13, 23, 23, 14, 1), ("2 Chronicles", 14, 1, 14, 14, 2),
    ("Job", 39, 1, 3, 38, 39), ("Job", 39, 4, 33, 39, 1), ("Job", 39, 34, 38, 40, 1),
    ("Job", 40, 1, 19, 40, 6), ("Job", 40, 20, 28, 41, 1), ("Job", 41, 1, 25, 41, 10),
    ("Ecclesiastes", 4, 17, 17, 5, 1), ("Ecclesiastes", 5, 1, 19, 5, 2),
    ("Ecclesiastes", 12, 1, 2, 11, 9), ("Ecclesiastes", 12, 3, 16, 12, 1),
    ("Song of Solomon", 7, 1, 1, 6, 13), ("Song of Solomon", 7, 2, 14, 7, 1),
    ("Isaiah", 8, 23, 23, 9, 1), ("Isaiah", 9, 1, 20, 9, 2), ("Isaiah", 64, 1, 11, 64, 2),
    ("Ezekiel", 21, 1, 5, 20, 45), ("Ezekiel", 21, 6, 37, 21, 1),
    ("Hosea", 2, 1, 2, 1, 10), ("Hosea", 2, 3, 25, 2, 1), ("Hosea", 12, 1, 1, 11, 12), ("Hosea", 12, 2, 15, 12, 1),
    ("Jonah", 2, 1, 1, 1, 17), ("Jonah", 2, 2, 11, 2, 1),
    ("Micah", 4, 14, 14, 5, 1), ("Micah", 5, 1, 14, 5, 2),
    ("Nahum", 2, 1, 1, 1, 15), ("Nahum", 2, 2, 14, 2, 1),
    ("2 Corinthians", 13, 13, 13, 13, 14),
]

# Verses the KJV holds as part of another: (book, chapter, verse) → joined onto (chapter, verse), after or before it.
JOINS = {
    ("1 Samuel", 20, 43): (20, 42, "after"),
    ("1 Kings", 22, 44): (22, 43, "after"),
    ("3 John", 1, 15): (1, 14, "after"),
    ("Revelation", 12, 18): (13, 1, "before"),
    # eBible's fraLSG ends Mark 10:52 as a verse 53 of its own.
    ("Mark", 10, 53): (10, 52, "after"),
}

# Verses the source runs together that the KJV keeps apart: (book, chapter, verse) → the words the next
# verse starts with. eBible's fraLSG joins Job 34:37 onto 34:36.
SPLITS = {
    ("Job", 34, 36): "Car il ajoute",
}


def kjv_place(book, chapter, verse):
    """The KJV (chapter, verse) of a pack verse, or None for a Psalm title."""
    if book == "Psalms" and chapter in PSALM_TITLE_VERSES:
        d = PSALM_TITLE_VERSES[chapter]
        return None if verse <= d else (chapter, verse - d)
    for b, c, a, z, kc, kv in RUNS:
        if b == book and c == chapter and a <= verse <= z:
            return (kc, kv + verse - a)
    return (chapter, verse)


def main(path):
    with open(path, encoding="utf-8") as f:
        pack = json.load(f, object_pairs_hook=OrderedDict)
    verses = pack["verses"]
    if not any(v["book"] == "Psalms" and v["chapter"] == 3 and v["verse"] == 9 for v in verses):
        print(f"{path}: already in KJV numbering")
        return
    placed = OrderedDict()
    joins = []
    for v in verses:
        key = (v["book"], v["chapter"], v["verse"])
        if key in JOINS:
            joins.append((key, v))
            continue
        where = kjv_place(*key)
        if where is None:
            continue
        nv = OrderedDict(v)
        nv["chapter"], nv["verse"] = where
        if "reference" in nv:
            nv["reference"] = f"{v['book']} {where[0]}:{where[1]}"
        placed[(v["book"], *where)] = nv
        at = nv["text"].find(SPLITS.get(key, "\0"))
        if at > 0:
            rest = OrderedDict(nv)
            nv["text"], rest["text"] = nv["text"][:at].rstrip(), nv["text"][at:]
            rest["verse"] = where[1] + 1
            if "reference" in rest:
                rest["reference"] = f"{v['book']} {where[0]}:{where[1] + 1}"
            placed[(v["book"], where[0], where[1] + 1)] = rest
    for (book, _, _), v in joins:
        c, n, side = JOINS[(book, v["chapter"], v["verse"])]
        target = placed.get((book, c, n))
        if target is not None:
            target["text"] = f"{target['text']} {v['text']}" if side == "after" else f"{v['text']} {target['text']}"
    pack["verses"] = list(placed.values())
    if "verseCount" in pack:
        pack["verseCount"] = len(pack["verses"])
    with open(path, "w", encoding="utf-8") as f:
        json.dump(pack, f, ensure_ascii=False, separators=(",", ":"))
    print(f"{path}: {len(verses)} → {len(placed)} verses in KJV numbering")


if __name__ == "__main__":
    main(sys.argv[1])
