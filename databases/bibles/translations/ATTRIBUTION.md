# English translation sources

Verse text for the "Translation" setting — swapped in by `apply_translation()` in
`kairo-server/src/bible.rs`. The KJV is the detection corpus's own text (`databases/bibles/kjv.json`);
the BSB below is the one other English Bible that ships, and the modern wording the detector matches
beside the KJV (word-by-word reading and meaning search).

| Code | Translation | Rights holder | Status |
|---|---|---|---|
| BSB | Berean Standard Bible | Berean Bible Translation Committee | Public domain (dedicated 2023) |

Source: https://bereanbible.com/bsb.txt (fetched 2026-10-30), converted to Kairo's flat
`{book, chapter, verse, reference, text}` schema (`Psalm` → `Psalms`, `Song of Songs` → `Song of
Solomon`); text unchanged.

Why the BSB: on the dev sermons (2026-10-30) matching the BSB beside the KJV found as many verses with
as few wrong ones as matching the NIV and NLT together (290 vs 291 of 347 found, 3 wrong each); the
World English Bible found as many with more wrong (5). A passage read word for word in another
translation's wording is no longer matched by that wording; a Bible the church adds is shown, not matched.

## Removed: NIV and NLT (2026-10)

NIV (Biblica) and NLT (Tyndale House Foundation; carried on `map.json`'s `nlt_text`) were removed:
copyrighted, with no licence for this project. `map.json` (KJV + NLT) went with them; the KJV comes
from `kjv.json`. History and releases up to 4.3.0 contain them.

## Removed: ESV and NASB (2026-10)

ESV (Crossway) and NASB (The Lockman Foundation) were removed: copyrighted, with no licence for this
project, and only ever shown, never used by the detector.

## Removed: NKJV (2026-10)

Its publisher (HarperCollins Christian Publishing / Thomas Nelson) declined permission in writing.

## Bibles a church adds

Under Settings → Bible → Your Bibles (API.Bible or a licensed file), under any code but KJV and BSB:
an NIV of its own takes the code NIV. A spoken "NIV" switches to that copy, else it is logged and
ignored. A saved choice of a Bible that is no longer there goes back to the KJV at startup.
