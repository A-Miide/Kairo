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
# The same songs with the lyrics slide up (the operator's normal flow): scripture heard is only ever offered.
for _n in ("psalm23", "isaiah40"):
    S["sung-live-" + _n] = S["sung-" + _n]
S_SONG = {"sung-live-psalm23", "sung-live-isaiah40"}
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
    ("so I say again, for God so loved the world, that he gave his only begotten Son", [("quote", "John 3:16")]),
]
# ── 4b. A partial quote of a long verse beside a short verse sharing its words ───────
S["rank-john-3-16-tail"] = [
    ("let us read John chapter three verse sixteen", [("citation", "John 3:16")]),
    ("that whosoever believeth in him should not perish, but have everlasting life", [("quote", "John 3:16")]),
]
S["rank-john-3-16-tail-uncited"] = [
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
S["fmt-twenty-third-psalm"] = [("let us go to the twenty third Psalm, the Lord is my shepherd, I shall not want", [("citation", "Psalm 23:1")])]
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
# ── 9. A chapter named with no verse: the words read next pick the verse ─────────────
S["chapter-then-words"] = [
    ("favour came on Solomon, look at first Kings three", []),
    ("and that which thou hast not asked I have also given thee, both riches and honour", [("quote", "1 Kings 3:13")]),
]
# The verse number comes on the next line: words from before the chapter must not pick one.
S["chapter-number-next"] = [
    ("Elijah prayed, the sky became black, he said get on your way before the rain stops you", []),
    ("you are getting speed tonight, first Kings eighteen and", []),
    ("forty six, and the hand of the Lord was on Elijah, and he ran before Ahab", [("citation", "1 Kings 18:46")]),
]
# ── 10. Read on from a cited verse, split as the live recogniser splits it (dev build log, 2026-10-19) ──
S["hold-daniel6-live"] = [
    ('Help me, help me. Daniel', []),
    ('chapter what? Chapter', []),
    ('six from verse three. I want you to', [("citation", "Daniel 6:3")]),
    ('see what happened there', []),
    ('Then this Daniel', []),
    ('was preferred above', []),
    ('the presidents and', []),
    ('princes because an excellent', []),
    ('spirit was in', []),
    ('him and the', []),
    ('king thought to set', []),
    ('him over the old realm.', []),
    ('Look at what', []),
    ('happened then they what', []),
    ('president and the princes', [("quote", "Daniel 6:4")]),
    ('sought to find', []),
    ('what occasion against', []),
    ('Daniel concerning the kingdom', []),
    ('but they could', []),
    ('not find none occasion', []),
    ('of all for', []),
    ('as much it', []),
    ('was faithful. I', []),
]
# ── 11. Wording shared by verses of one book ─────────────────────────────────────────
# Psalm 107's refrain is verses 8, 15, 21 and 31: reading on from 13 makes it 15.
S["same-psalm107-reading"] = [
    ("Psalm one hundred and seven verse thirteen", [("citation", "Psalm 107:13")]),
    ("then they cried unto the Lord in their trouble, and he saved them out of their distresses", [("quote", "Psalm 107:13")]),
    ("he brought them out of darkness and the shadow of death, and brake their bands in sunder", [("quote", "Psalm 107:14")]),
    ("oh that men would praise the Lord for his goodness, and for his wonderful works to the children of men", [("quote", "Psalm 107:15")]),
]
# The refrain said with nothing pointing at one of its four verses: nothing goes up.
S["same-psalm107-refrain-uncited"] = [
    ("church, oh that men would praise the Lord for his goodness, and for his wonderful works to the children of men", []),
]
S["same-proverbs14-cited"] = [
    ("Proverbs fourteen twelve", [("citation", "Proverbs 14:12")]),
    ("there is a way which seemeth right unto a man, but the end thereof are the ways of death", [("quote", "Proverbs 14:12")]),
]
# Proverbs 14:12 and 16:25 differ by one word ("which"/"that"): uncited, neither is sure.
S["same-proverbs-uncited"] = [
    ("there is a way that seemeth right unto a man, but the end thereof are the ways of death", []),
]
# Psalm 42:11 is nearly Psalm 43:5: the citation decides.
S["same-psalm42-11-cited"] = [
    ("Psalm forty two verse eleven", [("citation", "Psalm 42:11")]),
    ("why art thou cast down, O my soul? and why art thou disquieted within me? hope thou in God: for I shall yet praise him, who is the health of my countenance, and my God", [("quote", "Psalm 42:11")]),
]
# ── 12. Wording shared across books ──────────────────────────────────────────────────
# "The just shall live by faith": Habakkuk 2:4, Romans 1:17, Galatians 3:11, Hebrews 10:38.
S["cross-hebrews10-reading"] = [
    ("Hebrews chapter ten verse thirty six", [("citation", "Hebrews 10:36")]),
    ("for ye have need of patience, that, after ye have done the will of God, ye might receive the promise", [("quote", "Hebrews 10:36")]),
    ("for yet a little while, and he that shall come will come, and will not tarry", [("quote", "Hebrews 10:37")]),
    ("now the just shall live by faith: but if any man draw back, my soul shall have no pleasure in him", [("quote", "Hebrews 10:38")]),
]
S["cross-romans117-cited"] = [
    ("Romans one seventeen", [("citation", "Romans 1:17")]),
    ("for therein is the righteousness of God revealed from faith to faith: as it is written, the just shall live by faith", [("quote", "Romans 1:17")]),
]
S["cross-just-live-uncited"] = [
    ("and I want you to know this morning that the just shall live by faith", []),
]
# Acts 2:17 quotes Joel 2:28.
S["cross-acts217-cited"] = [
    ("Acts chapter two verse seventeen", [("citation", "Acts 2:17")]),
    ("and it shall come to pass in the last days, saith God, I will pour out of my Spirit upon all flesh: and your sons and your daughters shall prophesy", [("quote", "Acts 2:17")]),
]
S["cross-joel228-uncited"] = [
    ("and it shall come to pass afterward, that I will pour out my spirit upon all flesh; and your sons and your daughters shall prophesy, your old men shall dream dreams, your young men shall see visions", [("quote", "Joel 2:28")]),
]
# Matthew 24:35, Mark 13:31 and Luke 21:33 are word for word.
S["cross-heaven-earth-uncited"] = [
    ("heaven and earth shall pass away, but my words shall not pass away", []),
]
S["cross-hebrews13-cited"] = [
    ("Hebrews thirteen verse five", [("citation", "Hebrews 13:5")]),
    ("let your conversation be without covetousness; and be content with such things as ye have: for he hath said, I will never leave thee, nor forsake thee", [("quote", "Hebrews 13:5")]),
]
# Deuteronomy 31:6 and Joshua 1:5 end alike.
S["cross-forsake-uncited"] = [
    ("he will not fail thee, nor forsake thee", []),
]
# Micah 4:3 is nearly Isaiah 2:4.
S["cross-micah43-cited"] = [
    ("Micah four three", [("citation", "Micah 4:3")]),
    ("and they shall beat their swords into plowshares, and their spears into pruninghooks: nation shall not lift up a sword against nation, neither shall they learn war any more", [("quote", "Micah 4:3")]),
]
# ── 13. The room asked, the number given straight after ("chapter what twelve"), dev build 2026-10-20 ──
S["asked-john12-live"] = [
    ('Hear me', []),
    ('What number are', []),
    ('we now? Number what', []),
    ('number three that means', []),
    ('you are following me.', []),
    ('I want you', []),
    ('to pray. Can', []),
    ('I have the', []),
    ('Book of John', []),
    ('chapter what twelve from verse ten?', [("citation", "John 12:10")]),
    ('I want you to', []),
    ('see it one wickedness there and I want', []),
    ('you to relate', []),
    ('it here if you', []),
    ('understand what happened', []),
    ('in John chapter eleven,', []),
    ('then this script,', []),
    ('this verse should', []),
    ('strike you. He', []),
    ('said, but the chief priest', [("quote", "John 12:10")]),
    ('consulted that what', []),
    ('the same Lazarus that resurrected', []),
    ('am I making sense, you', []),
    ('know, there are', []),
    ('people that your congratulation', []),
    ('has started they', []),
    ('want to kill it', []),
]
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
    golden = {"source": f"scn-{name}", "kind": "scenario", "lang": S_LANG.get(name, "en"), "songLive": name in S_SONG, "cues": cues, "inputs": inputs, "stable": [], "unstable": [], "runs": []}
    fixture = {"videoId": f"scn-{name}", "title": f"Scenario: {name}", "transcript": [{"startMs": c["audioMs"], "text": c["text"]} for c in cues], "groundTruth": truth, "note": "eval/scenarios/make_scenarios.py"}
    json.dump(golden, open(os.path.join(ROOT, "parity/golden", f"scn-{name}.json"), "w"), indent=1, ensure_ascii=False)
    json.dump(fixture, open(os.path.join(ROOT, "eval/fixtures", f"scn-{name}.json"), "w"), indent=1, ensure_ascii=False)

if __name__ == "__main__":
    for n, l in S.items():
        build(n, l)
    print(len(S), "scenarios written")
