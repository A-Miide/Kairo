# Segmentation cases

The same words cut into segments differently, as a lagging recogniser cuts
them: a line starting `~ ` is an interim, any other line a final. What's
found must not depend on the cut (`docs/detection-spec/08-word-buffer.md`).

    cargo run --release -p kairo-eval --bin say -- eval/segmentation/psalm-67-live-log.txt --segments --explain

`kairo-detect/tests/segmentation.rs` checks the same texts split at every
word, as one utterance, as two, and with the first part heard only in an
interim.

Psalm 67:3 and 67:5 are worded alike (a refrain): read word for word with
the chapter named, 67:3 goes up and 67:5 is offered; the verse read next
tells which it was.
