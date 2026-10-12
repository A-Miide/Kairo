//! Builds the verse index the app ships (`databases/bibles/verse_index.<model>.kvx`)
//! if it isn't there yet (or was built for another numbering of the Bible).
//! Run before bundling.

use kairo_eval::{meaning, root};

fn main() {
    let target = root().join("databases/bibles/verse_index.gemma-legacy.kvx");
    let verses = kairo_bible::Corpus::load(&root().join("databases/bibles")).expect("kjv").len();
    // Present and built for this numbering of the Bible.
    if kairo_semantic::VerseIndex::load(&target).is_ok_and(|i| i.len() == verses) {
        println!("verse index present: {}", target.display());
        return;
    }
    // Builds (or reuses) the cached index, then copies it into place.
    let _ = meaning("gemma-legacy.json", None, &["KJV"]);
    let cached = root().join("eval-results/semantic-cache/gemma-legacy.KJV.kvx");
    std::fs::copy(&cached, &target).expect("copy verse index");
    println!("verse index built: {}", target.display());
}
