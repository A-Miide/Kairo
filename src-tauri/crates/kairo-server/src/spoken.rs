//! A translation the preacher asks for out loud ("Isaiah seven seven, NLT or Message") is for
//! that passage: reading on stays in it, and the next verse sent from another passage goes back to
//! the church's translation. The operator changing the translation themselves ends it too.

use std::sync::{Mutex, OnceLock};

struct Spoken {
    /// What the church had before the preacher asked.
    previous: String,
    /// Book and chapter it belongs to: the verse on screen when asked, else the next one sent.
    passage: Option<(String, u64)>,
}

static SPOKEN: Mutex<Option<Spoken>> = Mutex::new(None);
static ON_RESTORE: OnceLock<Box<dyn Fn(String) + Send + Sync>> = OnceLock::new();

/// What to do when the spoken translation ends (set once, at start): put `previous` back.
pub(crate) fn on_restore(f: impl Fn(String) + Send + Sync + 'static) {
    let _ = ON_RESTORE.set(Box::new(f));
}

/// The preacher asked for a translation; `previous` is what was set before.
pub(crate) fn begin(previous: String, passage: Option<(String, u64)>) {
    let mut s = crate::util::lock(&SPOKEN);
    // Asked again in the same breath ("NIV … no, NLT"): the church's own translation is still the one to go back to.
    let previous = s.take().map_or(previous, |old| old.previous);
    *s = Some(Spoken { previous, passage });
}

/// The operator chose a translation: nothing to go back to.
pub(crate) fn clear() {
    *crate::util::lock(&SPOKEN) = None;
}

/// A detected verse is going on screen. Returns the church's translation when it starts another
/// passage (the spoken one has ended, and the setting is being put back).
pub(crate) fn on_send(book: &str, chapter: u64) -> Option<String> {
    let mut s = crate::util::lock(&SPOKEN);
    let spoken = s.as_mut()?;
    match &spoken.passage {
        None => {
            spoken.passage = Some((book.to_string(), chapter));
            None
        }
        Some((b, c)) if b == book && *c == chapter => None,
        Some(_) => {
            let previous = s.take()?.previous;
            if let Some(f) = ON_RESTORE.get() {
                f(previous.clone());
            }
            Some(previous)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lasts_for_the_passage() {
        begin("KJV".into(), Some(("Isaiah".into(), 7)));
        assert_eq!(on_send("Isaiah", 7), None, "reading on stays in it");
        assert_eq!(on_send("John", 3), Some("KJV".into()), "another passage goes back");
        assert_eq!(on_send("Romans", 8), None, "and it's over");

        begin("KJV".into(), None);
        assert_eq!(on_send("John", 3), None, "the verse cited with the request");
        assert_eq!(on_send("John", 3), None);
        assert_eq!(on_send("Psalms", 23), Some("KJV".into()));

        begin("ESV".into(), Some(("Isaiah".into(), 7)));
        begin("NIV".into(), Some(("Isaiah".into(), 7)));
        assert_eq!(on_send("Mark", 1), Some("ESV".into()), "asked twice: back to the church's own");

        begin("KJV".into(), Some(("Isaiah".into(), 7)));
        clear();
        assert_eq!(on_send("John", 3), None, "the operator changed it");
    }
}
