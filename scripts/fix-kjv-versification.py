"""Restores standard KJV verse numbering in databases/bibles/kjv.json.

The bundled text was missing six verses (every later verse in those chapters
carried the wrong number) and split four verses in two (the critical-text
numbering). Wording checked against the KJV; the file's own conventions kept
(supplied words in braces, marginal notes at the end, "enquired").
Idempotent: a chapter already fixed is left alone.
"""
import json, pathlib, sys

PATH = pathlib.Path(__file__).resolve().parent.parent / "databases/bibles/kjv.json"
raw = PATH.read_bytes()
bom = raw.startswith(b"\xef\xbb\xbf")
bible = json.loads(raw.decode("utf-8-sig"))
book = {b["name"]: b for b in bible}

def chapter(name, c):
    return book[name]["chapters"][c - 1]

MISSING = [
    ("Matthew", 2, 16, 23, "Then Herod, when he saw that he was mocked of the wise men, was exceeding wroth, and sent forth, and slew all the children that were in Bethlehem, and in all the coasts thereof, from two years old and under, according to the time which he had diligently enquired of the wise men."),
    ("Matthew", 22, 1, 46, "And Jesus answered and spake unto them again by parables, and said,"),
    ("Matthew", 26, 38, 75, "Then saith he unto them, My soul is exceeding sorrowful, even unto death: tarry ye here, and watch with me."),
    ("Mark", 4, 40, 41, "And he said unto them, Why are ye so fearful? how is it that ye have no faith?"),
    ("Mark", 7, 11, 37, "But ye say, If a man shall say to his father or mother, {It is} Corban, that is to say, a gift, by whatsoever thou mightest be profited by me; {he shall be free}."),
    ("Mark", 8, 8, 38, "So they did eat, and were filled: and they took up of the broken meat that was left seven baskets."),
]
for name, c, v, verses, text in MISSING:
    ch = chapter(name, c)
    if len(ch) == verses:
        continue
    assert len(ch) == verses - 1, (name, c, len(ch))
    ch.insert(v - 1, text)

def split_note(s):
    """Text and a trailing marginal note ("{forasmuch...: or, ...}")."""
    if s.endswith("}") and "...:" in s[s.rfind("{"):]:
        i = s.rfind(" {")
        return s[:i], s[i:]
    return s, ""

# Verse v and its stray continuation, joined as the KJV numbers them.
JOINED = [("1 Samuel", 20, 42, 42, " "), ("1 Kings", 22, 43, 53, " "), ("3 John", 1, 14, 14, " ")]
for name, c, v, verses, sep in JOINED:
    ch = chapter(name, c)
    if len(ch) == verses:
        continue
    assert len(ch) == verses + 1, (name, c, len(ch))
    first, note1 = split_note(ch[v - 1])
    second, note2 = split_note(ch[v])
    if name == "1 Kings":
        first = first.rstrip()
        second = second[0].lower() + second[1:]
    ch[v - 1:v + 1] = [first + sep + second + note1 + note2]

# "And I stood upon the sand of the sea" begins Revelation 13:1 in the KJV.
rev12, rev13 = chapter("Revelation", 12), chapter("Revelation", 13)
if len(rev12) == 18:
    stray = rev12.pop().rstrip(".")
    rev13[0] = stray + ", and s" + rev13[0][len("And s"):]

total = sum(len(c) for b in bible for c in b["chapters"])
assert total == 31102, total
out = json.dumps(bible, ensure_ascii=False)
PATH.write_bytes((b"\xef\xbb\xbf" if bom else b"") + out.encode("utf-8"))
print(f"kjv.json: {total} verses")
