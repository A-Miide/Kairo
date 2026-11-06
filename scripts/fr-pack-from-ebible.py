#!/usr/bin/env python3
"""Builds the French pack (databases/bibles/packs/fr.json) from eBible.org's Louis Segond 1910, which
eBible publishes as public domain ("Cette Bible est dans le domaine public").

    curl -sSL -o fraLSG_vpl.zip https://ebible.org/Scriptures/fraLSG_vpl.zip && unzip fraLSG_vpl.zip
    python3 scripts/fr-pack-from-ebible.py fraLSG_vpl.txt databases/bibles/packs/fr.json
    python3 scripts/pack-versification.py databases/bibles/packs/fr.json

The second step moves Segond's numbering (Psalm titles as verse 1, Hebrew chapter breaks) to the KJV's,
which Kairo keys every verse by.
"""
import json
import sys

# eBible's VPL book codes, in canonical order, and the names Kairo uses.
BOOKS = [
    ("GEN", "Genesis"), ("EXO", "Exodus"), ("LEV", "Leviticus"), ("NUM", "Numbers"), ("DEU", "Deuteronomy"), ("JOS", "Joshua"),
    ("JDG", "Judges"), ("RUT", "Ruth"), ("1SA", "1 Samuel"), ("2SA", "2 Samuel"), ("1KI", "1 Kings"), ("2KI", "2 Kings"),
    ("1CH", "1 Chronicles"), ("2CH", "2 Chronicles"), ("EZR", "Ezra"), ("NEH", "Nehemiah"), ("EST", "Esther"), ("JOB", "Job"),
    ("PSA", "Psalms"), ("PRO", "Proverbs"), ("ECC", "Ecclesiastes"), ("SOL", "Song of Solomon"), ("ISA", "Isaiah"), ("JER", "Jeremiah"),
    ("LAM", "Lamentations"), ("EZE", "Ezekiel"), ("DAN", "Daniel"), ("HOS", "Hosea"), ("JOE", "Joel"), ("AMO", "Amos"), ("OBA", "Obadiah"),
    ("JON", "Jonah"), ("MIC", "Micah"), ("NAH", "Nahum"), ("HAB", "Habakkuk"), ("ZEP", "Zephaniah"), ("HAG", "Haggai"), ("ZEC", "Zechariah"),
    ("MAL", "Malachi"), ("MAT", "Matthew"), ("MAR", "Mark"), ("LUK", "Luke"), ("JOH", "John"), ("ACT", "Acts"), ("ROM", "Romans"),
    ("1CO", "1 Corinthians"), ("2CO", "2 Corinthians"), ("GAL", "Galatians"), ("EPH", "Ephesians"), ("PHI", "Philippians"),
    ("COL", "Colossians"), ("1TH", "1 Thessalonians"), ("2TH", "2 Thessalonians"), ("1TI", "1 Timothy"), ("2TI", "2 Timothy"),
    ("TIT", "Titus"), ("PHM", "Philemon"), ("HEB", "Hebrews"), ("JAM", "James"), ("1PE", "1 Peter"), ("2PE", "2 Peter"), ("1JO", "1 John"),
    ("2JO", "2 John"), ("3JO", "3 John"), ("JUD", "Jude"), ("REV", "Revelation"),
]


def main(src, out):
    names = dict(BOOKS)
    verses = []
    with open(src, encoding="utf-8-sig") as f:
        for line in f:
            line = line.rstrip("\n")
            if not line.strip():
                continue
            code, ref, text = line.split(" ", 2)
            chapter, verse = ref.split(":")
            book = names[code]
            verses.append({"book": book, "chapter": int(chapter), "verse": int(verse), "reference": f"{book} {chapter}:{verse}", "text": text.strip()})
    pack = {
        "language": "French",
        "translation": "Louis Segond (1910)",
        "license": "Public domain",
        "source": "https://ebible.org/Scriptures/fraLSG_vpl.zip",
        "verseCount": len(verses),
        "verses": verses,
    }
    with open(out, "w", encoding="utf-8") as f:
        json.dump(pack, f, ensure_ascii=False, separators=(",", ":"))
    print(f"{out}: {len(verses)} verses from {src}")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
