# Handover: detector review, 4.6.3 and natural language (14 Nov 2026)

4.6.3 is merged (pull request #16) and tagged `v4.6.3`; its draft release builds from the tag. **Not published.** The server changes were checked by unit tests and replays only, never in the running app: install the draft and run a short service before publishing, and replace the notes in `latest.json` as every release.

## In 4.6.3

- Meaning answers matched to their request by id; words still being said don't wait for the meaning models.
- A new or restarted engine keeps the verse on the output; the singing classifier restarts with listening.
- Detection survives a panic while turning decisions into messages; restarts, slow meaning and dropped audio are reported.
- Offline engine: no repeated speech after a restart; restarted when it stops answering.
- Corroboration means any other method. A meaning match vouches only for a quote of the verse's own words (owner's choice).
- Hidden offer kinds decided in the engine (replayable); one "on screen since" clock.
- Paraphrases: the surest go up only inside the passage the sermon is on; otherwise offered. The scoped meaning search is never sent.

Owner's rules, 14 Nov: no wrong sends from paraphrase or natural language; scripture said in a prayer or an instruction stays as it is (sent when it is the verse's exact words).

## After 4.6.3 (branch `fix/paraphrase-pause`)

- A paraphrase followed by a pause is decided on that one search (it used to need a second search that never came).
- Every meaning search also tries the last 14 words, so a verse said in passing inside a long stretch without sentence breaks has a window of its own.
- Two verses worded alike in successive searches count as agreement (Deuteronomy 31:6 / 1 Chronicles 28:20 took turns at the top).
- "Third John five verse nineteen": a number said before a book is never dropped for the plain book; 1 John 5:19, not John 5:19.

## Roadmap

1. **Other languages:** a very sure meaning match still goes up with no passage in play. Bring it to the English rule (owner, 14 Nov: roadmap, not now).
2. **Paraphrase offers are hidden live.** Possible Matches shows only kinds right at least 75% of the time (owner, 13 Nov); paraphrase measured 63%, so a paraphrase offer is seen only when another kind agrees. Improving paraphrase offers changes little live until that measurement improves or the rule changes.
3. **Flowing natural language:** a sure meaning match the preacher moves straight on from holds for one search only, and is lost (Deuteronomy 31:6 on the 24 Sep clip at 0.986, then 0.953). Lowering the bar was tried in October and let noise in.
4. **Quote cue plus modern wording:** "Like Paul said, I can do everything through him who gives me strength" is only offered. A cue ("the Bible says", a named apostle) could lift the hold; measure wrong sends first.
5. **"Second John one twelve"** gives nothing ("one twelve" is read as one number); "Second John one verse twelve" works.
6. **The sermon block:** one model of the passages in play, the kind of speech (reading, quote, prayer, story, instruction) and who is spoken of, replacing the four context timers. Build it against a **sermon simulator** made from `eval/paraphrase_cases*.json` (hunting real sermons for natural language is futile, owner), then an opt-in "Kairo missed this" and per-church learning on the device.
7. **Windows hardware:** still not tried (open since 4.6.2).

## Test material

- `~/Downloads/fgwL6uXkLDXP_xfQ.mp4` (24 Sep): references and natural language at speed; transcribed with Core ML into `parity/offline/clip-fgwL6uXkLDXP_xfQ.json` (not committed: no ground truth yet).
- `~/Downloads/6saCUb9lcaOQ72N1.mp4` (9 Nov): verses quoted in the preacher's own order; handled.
- `say <file> --meaning bge-small.json --hits` prints what each meaning search found.
