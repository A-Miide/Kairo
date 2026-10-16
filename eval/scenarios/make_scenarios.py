#!/usr/bin/env python3
"""Detection scenario pack: short scripted services for situations the sermon fixtures don't
cover well. Each scenario becomes a recording (parity/golden/scn-<name>.json, growing interim
text then a final per line, like a live run) and an answer key (eval/fixtures/scn-<name>.json),
so `replay scn-<name>` scores it like any sermon: found / wrong / offered.

  python3 eval/scenarios/make_scenarios.py        then   replay --verbose $(ls parity/golden | grep ^scn- | sed 's/.json//')

A line is (text, [answers said on that line]). An answer is (kind, "Book C:V" or "Book C:V-W").
No answers on a line means nothing should go on screen from it.
"""
import json, os, re
ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
MS_PER_WORD = 400
GAP_MS = 900

S = {}
# ── 1. Scripture sung in worship: nothing should go on screen by itself ───────────────
S["sung-psalm23"] = [
    ("the Lord is my shepherd, I shall not want, the Lord is my shepherd, I shall not want", []),
    ("he maketh me to lie down in green pastures, he leadeth me beside the still waters", []),
    ("he restoreth my soul, he restoreth my soul, hallelujah", []),
    ("surely goodness and mercy shall follow me all the days of my life", []),
]
S["sung-isaiah40"] = [
    ("they that wait upon the Lord shall renew their strength", []),
    ("they shall mount up with wings as eagles, they shall run and not be weary", []),
    ("they shall walk and not faint, teach me Lord, teach me Lord to wait", []),
]
# ── 2. Misspoken citations: the words read settle it ─────────────────────────────────
S["slip-jonah-john"] = [
    ("open your Bible to John chapter one verse seventeen", []),
    ("now the Lord had prepared a great fish to swallow up Jonah, and Jonah was in the belly of the fish three days and three nights", [("quote", "Jonah 1:17")]),
]
S["slip-judges-jude"] = [
    ("look at Jude chapter six verse twelve", []),
    ("and the angel of the Lord appeared unto him, and said unto him, The Lord is with thee, thou mighty man of valour", [("quote", "Judges 6:12")]),
]
S["slip-philemon"] = [
    ("Philemon four thirteen, say it with me", []),
    ("I can do all things through Christ which strengtheneth me", [("quote", "Philippians 4:13")]),
]
S["slip-corrected-aloud"] = [
    ("turn with me to Romans chapter eight, sorry, Romans chapter twelve verse one", [("citation", "Romans 12:1")]),
    ("I beseech you therefore, brethren, by the mercies of God, that ye present your bodies a living sacrifice", [("quote", "Romans 12:1")]),
]
S["slip-wrong-chapter"] = [
    ("Matthew five verse thirty three, the Bible says", []),
    ("but seek ye first the kingdom of God, and his righteousness; and all these things shall be added unto you", [("quote", "Matthew 6:33")]),
]
# ── 4. Recall: a verse from earlier interrupts the reading, then the reading resumes ──
S["recall-and-return"] = [
    ("let us read John chapter three verse sixteen", [("citation", "John 3:16")]),
    ("for God so loved the world, that he gave his only begotten Son", [("quote", "John 3:16")]),
    ("remember John eleven thirty five we read last week, Jesus wept", [("citation", "John 11:35")]),
    ("he felt what you feel church, he knows your pain, he has been there, he understands", []),
    ("that is why I am telling you this morning that his love is real and his love is personal", []),
    ("that whosoever believeth in him should not perish, but have everlasting life", [("quote", "John 3:16")]),
]
# ── 5. Reading a modern translation ──────────────────────────────────────────────────
S["niv-john316-uncited"] = [
    ("for God so loved the world that he gave his one and only Son", []),
    ("that whoever believes in him shall not perish but have eternal life", [("quote", "John 3:16")]),
]
S["niv-romans828-cited"] = [
    ("Romans eight twenty eight in the New International Version", [("citation", "Romans 8:28")]),
    ("and we know that in all things God works for the good of those who love him, who have been called according to his purpose", [("quote", "Romans 8:28")]),
]
S["niv-philippians4-uncited"] = [
    ("do not be anxious about anything, but in every situation, by prayer and petition, with thanksgiving, present your requests to God", [("quote", "Philippians 4:6")]),
    ("and the peace of God, which transcends all understanding, will guard your hearts and your minds in Christ Jesus", [("quote", "Philippians 4:7")]),
]
S["nkjv-psalm23-uncited"] = [
    ("the Lord is my shepherd, I shall not want, he makes me to lie down in green pastures", [("quote", "Psalm 23:1"), ("quote", "Psalm 23:2")]),
    ("he leads me beside the still waters, he restores my soul", [("quote", "Psalm 23:2"), ("quote", "Psalm 23:3")]),
]
# ── 6. Skipping verses while reading ─────────────────────────────────────────────────
S["skip-said"] = [
    ("Romans chapter eight verse twenty eight", [("citation", "Romans 8:28")]),
    ("and we know that all things work together for good to them that love God, to them who are the called according to his purpose", [("quote", "Romans 8:28")]),
    ("now drop down to verse thirty one", [("citation", "Romans 8:31")]),
    ("what shall we then say to these things? If God be for us, who can be against us", [("quote", "Romans 8:31")]),
]
S["skip-silent"] = [
    ("Romans chapter eight verse twenty eight", [("citation", "Romans 8:28")]),
    ("and we know that all things work together for good to them that love God, to them who are the called according to his purpose", [("quote", "Romans 8:28")]),
    ("what shall we then say to these things? If God be for us, who can be against us", [("quote", "Romans 8:31")]),
]
# ── 7. How references are said ───────────────────────────────────────────────────────
S["fmt-twenty-third-psalm"] = [("let us go to the twenty third Psalm, the Lord is my shepherd", [("citation", "Psalm 23:1")])]
S["fmt-psalm119-105"] = [("Psalm one hundred and nineteen verse one hundred and five", [("citation", "Psalm 119:105")])]
S["fmt-from-down-to"] = [("Romans eight from twenty eight down to thirty one", [("citation", "Romans 8:28-31")])]
S["fmt-through"] = [("first Corinthians thirteen four through seven", [("citation", "1 Corinthians 13:4-7")])]
S["fmt-second-kings"] = [("second Kings chapter two verse nine", [("citation", "2 Kings 2:9")])]
S["fmt-cross-chapter"] = [("John three thirty six to chapter four verse two", [("citation", "John 3:36")])]
# ── 8. Verse numbers however they're said ────────────────────────────────────────────
S["num-mixed-digits"] = [("Romans eight verse twenty-8", [("citation", "Romans 8:28")])]
S["num-pidgin-aside"] = [("Romans chapter eight, abeg, verse twenty eight", [("citation", "Romans 8:28")])]
# (A scenario's language: S_LANG; the engine runs in it, as Settings sets it for a service.)
S["num-spanish"] = [("Romanos ocho versículo veintiocho", [("citation", "Romans 8:28")])]
S["num-french"] = [("Romains huit verset vingt-huit", [("citation", "Romans 8:28")])]
S["num-portuguese"] = [("Romanos oito versículo vinte e oito", [("citation", "Romans 8:28")])]
S["num-german"] = [("Römer acht Vers achtundzwanzig", [("citation", "Romans 8:28")])]
S["num-spanish-mixed"] = [("Romanos ocho versículo veinti 8", [("citation", "Romans 8:28")])]
S_LANG = {"num-spanish": "es", "num-french": "fr", "num-portuguese": "pt", "num-german": "de", "num-spanish-mixed": "es"}

def answer(kind, ref, at):
    book, cv = ref.rsplit(" ", 1)
    c, v = cv.split(":")
    a = {"book": book, "chapter": int(c), "kind": kind, "startMs": at}
    if "-" in v:
        s, e = v.split("-"); a["verseStart"], a["verseEnd"] = int(s), int(e)
    else:
        a["verse"] = int(v)
    return a

def build(name, lines):
    cues, inputs, truth, t = [], [], [], 0
    for i, (text, answers) in enumerate(lines):
        words = text.split()
        cues.append({"text": text, "audioMs": t})
        for n in range(4, len(words), 4):
            inputs.append({"cue": i, "text": " ".join(words[:n]), "isFinal": False})
        inputs.append({"cue": i, "text": text, "isFinal": True})
        for k, r in answers:
            a = answer(k, r, t)
            # One showing answers a citation and the reading that follows it (as in the sermon
            # keys); the same verse again 30 s or more later (a return) needs a showing of its own.
            same = [x for x in truth if (x["book"], x["chapter"], x.get("verse", x.get("verseStart"))) == (a["book"], a["chapter"], a.get("verse", a.get("verseStart")))]
            if same and t - same[-1]["startMs"] < 30000:
                continue
            truth.append(a)
        t += len(words) * MS_PER_WORD + GAP_MS
    golden = {"source": f"scn-{name}", "kind": "scenario", "lang": S_LANG.get(name, "en"), "cues": cues, "inputs": inputs, "stable": [], "unstable": [], "runs": []}
    fixture = {"videoId": f"scn-{name}", "title": f"Scenario: {name}", "transcript": [{"startMs": c["audioMs"], "text": c["text"]} for c in cues], "groundTruth": truth, "note": "eval/scenarios/make_scenarios.py"}
    json.dump(golden, open(os.path.join(ROOT, "parity/golden", f"scn-{name}.json"), "w"), indent=1, ensure_ascii=False)
    json.dump(fixture, open(os.path.join(ROOT, "eval/fixtures", f"scn-{name}.json"), "w"), indent=1, ensure_ascii=False)

if __name__ == "__main__":
    for n, l in S.items():
        build(n, l)
    print(len(S), "scenarios written")
