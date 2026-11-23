#!/usr/bin/env python3
"""Profiles Bible versions for what makes live detection go wrong, without copying any text out.

For each version (the KJV, the BSB, and every flat Bible in the folders given, such as the ones
installed in the Kairo app), it counts and lists by reference only:

  coverage     verses present, missing or empty against the KJV's 31,102
  lookalikes   pairs of verses sharing a rare stretch of about 7 words, a word in it allowed to differ
               (three rare 5-word windows with at most one word different), and how many of those the
               detector's twin table (4-word phrases, exact) would miss
  numbers      verses with a number spoken in their words ("fifty and six"), and those where it is
               also a verse number of their own chapter, so it can be heard as "verse six"
  openings     verses whose first words also come, in order, inside the verse before (the next
               verse's opening said before its time) and verses opening exactly like the next one
  short        verses of four words or fewer
  book words   verses with a word that is also a book's name ("mark", "acts", "job", "numbers")
  vocabulary   the share of the version's words the KJV never uses (how unlike the KJV it reads)

Usage:
  python3 -I scripts/bible-profile.py [--bibles <folder>]... [--out report.json] [--examples 15]
The default folders are the app's translations folder on this Mac. The report holds references and
counts only; it is written where --out says (default: the system temp folder).
"""
import argparse
import json
import os
import re
import sys
import tempfile
from collections import Counter, defaultdict

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
APP_TRANSLATIONS = os.path.expanduser("~/Library/Application Support/com.kairo.scripture/translations")

STOP = set("""a an and are as at be but by for from had has have he her him his i if in into is it its me my
no not of on or our shall she so that the their them then there they this thou thee thy to unto up upon us
was we were what when which who will with ye you your said says say also all""".split())
NUMBER_WORDS = set("""one two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen
sixteen seventeen eighteen nineteen twenty thirty forty fifty sixty seventy eighty ninety hundred""".split())
UNITS = {w: i + 1 for i, w in enumerate("one two three four five six seven eight nine".split())}
TEENS = {w: i + 10 for i, w in enumerate("ten eleven twelve thirteen fourteen fifteen sixteen seventeen eighteen nineteen".split())}
TENS = {w: (i + 2) * 10 for i, w in enumerate("twenty thirty forty fifty sixty seventy eighty ninety".split())}
BOOK_WORDS = set("""genesis exodus leviticus numbers deuteronomy joshua judges ruth samuel kings chronicles ezra
nehemiah esther job psalms psalm proverbs ecclesiastes song isaiah jeremiah lamentations ezekiel daniel hosea
joel amos obadiah jonah micah nahum habakkuk zephaniah haggai zechariah malachi matthew mark luke john acts
romans corinthians galatians ephesians philippians colossians thessalonians timothy titus philemon hebrews
james peter jude revelation""".split())
# Book names whose plain sense is common in the text: the ones a reading can be heard as a citation by.
COMMON_SENSE_BOOK_WORDS = {"numbers", "judges", "kings", "chronicles", "acts", "mark", "job", "song", "lamentations", "revelation", "proverbs", "psalms", "psalm"}

# Shared rare 5-word windows (one word may differ) that make two verses look alike: three is a
# stretch of about seven words. One or two are everyday phrasing (sampled by hand, 23 Nov 2026).
LOOKALIKE_MIN_WINDOWS = 3

WORD = re.compile(r"[a-z0-9]+(?:'[a-z]+)?")


def tokens(text):
    text = re.sub(r"\{[^}]*\}", lambda m: m.group(0)[1:-1], text or "")
    return WORD.findall(text.lower().replace("’", "'"))


def stem(w):
    for suf in ("eth", "est", "ing", "ed", "es", "s"):
        if len(w) > len(suf) + 2 and w.endswith(suf):
            return w[: -len(suf)]
    return w


def load_kjv():
    books = json.load(open(os.path.join(ROOT, "databases/bibles/kjv.json"), encoding="utf-8-sig"))
    names = book_names()
    out = {}
    for b, book in enumerate(books):
        for c, chapter in enumerate(book["chapters"], 1):
            for v, text in enumerate(chapter, 1):
                out[(names[b], c, v)] = text
    return out


def book_names():
    bsb = json.load(open(os.path.join(ROOT, "databases/bibles/translations/BSB.json"), encoding="utf-8-sig"))
    names = []
    for x in bsb["verses"]:
        if not names or names[-1] != x["book"]:
            names.append(x["book"])
    return names


def load_flat(path):
    d = json.load(open(path, encoding="utf-8-sig"))
    out = {}
    for x in d.get("verses", []):
        ref = x.get("reference", "")
        verse = x.get("verse")
        if verse is None:
            m = re.search(r":(\d+)$", ref)
            verse = int(m.group(1)) if m else None
        if verse is None:
            continue
        out[(x["book"], int(x["chapter"]), int(verse))] = x.get("text", "")
    return out


def label(ref):
    return f"{ref[0]} {ref[1]}:{ref[2]}"


def number_value(words):
    """The number a run of number words says ("fifty and six" -> 56), or None."""
    total, seen = 0, False
    for w in words:
        if w == "and":
            continue
        if w in TENS:
            total += TENS[w]
        elif w in TEENS:
            total += TEENS[w]
        elif w in UNITS:
            total += UNITS[w]
        else:
            return None
        seen = True
    return total if seen else None


def lookalikes(verses, max_df=6, run=5):
    """Pairs of verses sharing a rare run of `run` words (stems) with at most one word different."""
    keys = defaultdict(set)
    for ref, text in verses.items():
        t = [stem(w) for w in tokens(text)]
        for i in range(len(t) - run + 1):
            window = t[i : i + run]
            if sum(1 for w in window if w not in STOP) < 2:
                continue
            for hole in range(run):
                keys[tuple(w if j != hole else "*" for j, w in enumerate(window))].add(ref)
    pairs = Counter()
    for refs in keys.values():
        if 1 < len(refs) <= max_df:
            rs = sorted(refs)
            for i in range(len(rs)):
                for j in range(i + 1, len(rs)):
                    pairs[(rs[i], rs[j])] += 1
    return pairs


def exact_twins(verses, max_df=6):
    """The detector's own twin rule, roughly: two or more shared 4-word phrases each in at most 6 verses."""
    grams = defaultdict(set)
    for ref, text in verses.items():
        t = [stem(w) for w in tokens(text)]
        for i in range(len(t) - 3):
            grams[tuple(t[i : i + 4])].add(ref)
    shared = Counter()
    for refs in grams.values():
        if 1 < len(refs) <= max_df:
            rs = sorted(refs)
            for i in range(len(rs)):
                for j in range(i + 1, len(rs)):
                    shared[(rs[i], rs[j])] += 1
    return {p for p, n in shared.items() if n >= 2}


def profile(name, verses, kjv, kjv_vocab, kjv_pairs, chapter_len, n_examples):
    refs = sorted(verses, key=lambda r: (BOOK_ORDER.get(r[0], 99), r[1], r[2]))
    present = [r for r in refs if (verses[r] or "").strip()]
    missing = [r for r in kjv if r not in verses]
    empty = [r for r in refs if not (verses[r] or "").strip()]
    toks = {r: tokens(verses[r]) for r in present}

    pairs = lookalikes({r: verses[r] for r in present})
    near = {p for p, n in pairs.items() if n >= LOOKALIKE_MIN_WINDOWS}
    # Same chapter, neighbouring verses repeating themselves aren't a cross-reference risk worth listing.
    far = {p for p in near if not (p[0][:2] == p[1][:2] and abs(p[0][2] - p[1][2]) <= 1)}
    twins = exact_twins({r: verses[r] for r in present})
    missed_by_twins = {p for p in far if p not in twins}
    other_books = {p for p in far if p[0][0] != p[1][0]}
    not_in_kjv = {p for p in far if p not in kjv_pairs} if kjv_pairs is not None else set()

    numbered, own_chapter_number = [], []
    for r, t in toks.items():
        runs, cur = [], []
        for w in t + ["."]:
            if w in NUMBER_WORDS or (w == "and" and cur):
                cur.append(w)
            else:
                if cur:
                    runs.append(cur[:-1] if cur[-1] == "and" else cur)
                cur = []
        values = [number_value(x) for x in runs if x]
        values = [v for v in values if v]
        if values:
            numbered.append(r)
            last_words = [number_value(x[-1:]) for x in runs if x]
            if any(v and v <= chapter_len.get(r[:2], 0) for v in values + last_words):
                own_chapter_number.append(r)

    early_opening, same_opening = [], []
    for r in present:
        nxt = (r[0], r[1], r[2] + 1)
        if nxt not in toks or r not in toks:
            continue
        a, b = toks[r], toks[nxt]
        if len(b) >= 3 and any(a[i : i + 3] == b[:3] for i in range(1, max(1, len(a) - 3))):
            early_opening.append(nxt)
        if len(a) >= 3 and len(b) >= 3 and a[:3] == b[:3]:
            same_opening.append(nxt)

    short = [r for r, t in toks.items() if len(t) <= 4]
    book_words = [r for r, t in toks.items() if any(w in COMMON_SENSE_BOOK_WORDS for w in t)]

    vocab = Counter(w for t in toks.values() for w in t)
    unseen_types = [w for w in vocab if w not in kjv_vocab]
    unseen_share = sum(vocab[w] for w in unseen_types) / max(1, sum(vocab.values()))

    def ex(items):
        return [label(x) if isinstance(x[0], str) else f"{label(x[0])} ~ {label(x[1])}" for x in list(items)[:n_examples]]

    def top_pairs(ps):
        return [f"{label(a)} ~ {label(b)}" for (a, b) in sorted(ps, key=lambda p: -pairs[p])[:n_examples]]

    return {
        "version": name,
        "coverage": {"verses": len(present), "missing_vs_kjv": len(missing), "empty": len(empty), "missing_examples": ex(missing)},
        "lookalikes": {
            "pairs": len(far),
            "across_books": len(other_books),
            "missed_by_twin_table": len(missed_by_twins),
            "not_lookalike_in_kjv": len(not_in_kjv),
            "verses_with_a_lookalike": len({r for p in far for r in p}),
            "examples_missed_by_twin_table": top_pairs(missed_by_twins),
            "examples_not_in_kjv": top_pairs(not_in_kjv),
        },
        "numbers": {"verses": len(numbered), "can_be_heard_as_a_verse_number": len(own_chapter_number), "examples": ex(own_chapter_number)},
        "openings": {
            "next_verse_opening_inside_this_one": len(early_opening),
            "same_opening_as_next": len(same_opening),
            "examples_inside": ex(early_opening),
        },
        "short_verses": {"count": len(short), "examples": ex(short)},
        "book_name_words": {"verses": len(book_words), "examples": ex(book_words)},
        "vocabulary": {"distinct_words": len(vocab), "words_kjv_never_uses": len(unseen_types), "share_of_running_words": round(unseen_share, 3)},
    }


BOOK_ORDER = {}


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--bibles", action="append", help="folder of flat <CODE>.json Bibles (repeatable)")
    ap.add_argument("--out", default=os.path.join(tempfile.gettempdir(), "bible-profile.json"))
    ap.add_argument("--examples", type=int, default=15)
    args = ap.parse_args()

    BOOK_ORDER.update({b: i for i, b in enumerate(book_names())})
    kjv = load_kjv()
    chapter_len = Counter()
    for (b, c, v) in kjv:
        chapter_len[(b, c)] = max(chapter_len[(b, c)], v)
    kjv_vocab = {w for t in kjv.values() for w in tokens(t)}

    versions = [("KJV", kjv), ("BSB", load_flat(os.path.join(ROOT, "databases/bibles/translations/BSB.json")))]
    for folder in args.bibles or [APP_TRANSLATIONS]:
        if os.path.isdir(folder):
            for f in sorted(os.listdir(folder)):
                if f.lower().endswith(".json"):
                    versions.append((os.path.splitext(f)[0].upper(), load_flat(os.path.join(folder, f))))

    kjv_pairs = None
    report = []
    for name, verses in versions:
        print(f"profiling {name}…", file=sys.stderr)
        p = profile(name, verses, kjv, kjv_vocab, kjv_pairs, chapter_len, args.examples)
        if name == "KJV":
            kjv_pairs = set()
            # The KJV's own look-alikes, for "not a look-alike in the KJV" in the others.
            kp = lookalikes({r: t for r, t in kjv.items() if (t or "").strip()})
            kjv_pairs = {q for q, n in kp.items() if n >= LOOKALIKE_MIN_WINDOWS}
            p["lookalikes"]["not_lookalike_in_kjv"] = 0
        report.append(p)

    with open(args.out, "w") as f:
        json.dump(report, f, indent=1)

    row = "{:<6} {:>7} {:>7} {:>10} {:>9} {:>12} {:>9} {:>11} {:>9} {:>6} {:>9} {:>9}"
    print(row.format("", "verses", "missing", "lookalike", "missed by", "lookalike", "numbers", "number can", "opening", "short", "book-name", "words not"))
    print(row.format("", "", "", "pairs", "twin tbl", "not in KJV", "in text", "be a verse", "inside", "", "words", "in KJV"))
    for p in report:
        print(row.format(p["version"], p["coverage"]["verses"], p["coverage"]["missing_vs_kjv"], p["lookalikes"]["pairs"], p["lookalikes"]["missed_by_twin_table"],
                         p["lookalikes"]["not_lookalike_in_kjv"], p["numbers"]["verses"], p["numbers"]["can_be_heard_as_a_verse_number"],
                         p["openings"]["next_verse_opening_inside_this_one"], p["short_verses"]["count"], p["book_name_words"]["verses"],
                         f'{p["vocabulary"]["share_of_running_words"] * 100:.1f}%'))
    print(f"\nreport: {args.out}")


if __name__ == "__main__":
    main()
