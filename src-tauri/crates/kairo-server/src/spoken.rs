//! A translation the preacher asks for out loud ("Isaiah seven seven, NLT or Message") is for
//! that passage: reading on stays in it, a verse from another passage goes up in the church's
//! translation, and coming back to the passage brings the asked-for one back ("go from verse twenty
//! one, give me the NLT", a word on Matthew 5:45, then the reading of Ezekiel 12 in the NLT: live,
//! 2026-10-27). The operator changing the translation themselves ends it, and so does a new
//! passage cited once the asked-for one has been left a while.

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Away from the passage this long, the asked-for translation is over.
const AWAY_ENDS: Duration = Duration::from_secs(5 * 60);

struct Spoken {
    /// What the church had before the preacher asked.
    previous: String,
    /// What the preacher asked for.
    asked: String,
    /// Book and chapter it belongs to: the verse on screen when asked, else the next one sent.
    passage: Option<(String, u64)>,
    /// Since when a verse from another passage has been up (in the church's translation).
    away: Option<Instant>,
}

static SPOKEN: Mutex<Option<Spoken>> = Mutex::new(None);
static ON_RESTORE: OnceLock<Box<dyn Fn(String) + Send + Sync>> = OnceLock::new();

/// What to do when the translation must change for the verse going up (set once, at start).
pub(crate) fn on_restore(f: impl Fn(String) + Send + Sync + 'static) {
    let _ = ON_RESTORE.set(Box::new(f));
}

/// The preacher asked for `asked`; `previous` is what was set before.
pub(crate) fn begin(previous: String, asked: &str, passage: Option<(String, u64)>) {
    let mut s = crate::util::lock(&SPOKEN);
    // Asked again in the same breath ("NIV … no, NLT"): the church's own translation is still the one to go back to.
    let previous = s.take().map_or(previous, |old| old.previous);
    *s = Some(Spoken { previous, asked: asked.to_string(), passage, away: None });
}

/// The operator chose a translation: nothing to go back to.
pub(crate) fn clear() {
    *crate::util::lock(&SPOKEN) = None;
}

/// The translation a verse about to go up should be shown in, when it isn't the current one (what
/// `on_send` will switch to), without changing anything: detection puts the verse in it before
/// sending, and the switch itself happens once it's on screen (hub.rs), so the repaint lands on the
/// new verse and nothing on screen flickers through the other translation (review).
pub(crate) fn translation_for(book: &str, chapter: u64) -> Option<String> {
    let s = crate::util::lock(&SPOKEN);
    let spoken = s.as_ref()?;
    match &spoken.passage {
        None => None,
        Some((b, c)) if b == book && *c == chapter => spoken.away.map(|_| spoken.asked.clone()),
        Some(_) => match spoken.away {
            Some(since) if since.elapsed() >= AWAY_ENDS => None,
            Some(_) => None,
            None => Some(spoken.previous.clone()),
        },
    }
}

/// A verse went on screen. Returns the translation it goes up in when that changes: the
/// church's for another passage, the asked-for one back in its passage.
pub(crate) fn on_send(book: &str, chapter: u64) -> Option<String> {
    let mut s = crate::util::lock(&SPOKEN);
    let spoken = s.as_mut()?;
    let change = match &spoken.passage {
        None => {
            spoken.passage = Some((book.to_string(), chapter));
            None
        }
        Some((b, c)) if b == book && *c == chapter => spoken.away.take().map(|_| spoken.asked.clone()),
        Some(_) => match spoken.away {
            Some(since) if since.elapsed() >= AWAY_ENDS => {
                // Left a while: a new passage, and the church's translation stays.
                *s = None;
                None
            }
            Some(_) => None,
            None => {
                spoken.away = Some(Instant::now());
                Some(spoken.previous.clone())
            }
        },
    };
    drop(s);
    if let (Some(code), Some(f)) = (change.as_ref(), ON_RESTORE.get()) {
        f(code.clone());
    }
    change
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lasts_for_the_passage() {
        begin("KJV".into(), "NLT", Some(("Isaiah".into(), 7)));
        assert_eq!(on_send("Isaiah", 7), None, "reading on stays in it");
        assert_eq!(on_send("John", 3), Some("KJV".into()), "another passage goes up in the church's");
        assert_eq!(on_send("Romans", 8), None, "still away");
        assert_eq!(translation_for("Isaiah", 7), Some("NLT".into()), "told before it's sent");
        assert_eq!(on_send("Isaiah", 7), Some("NLT".into()), "back in the passage, back in what was asked for");
        assert_eq!(on_send("Isaiah", 7), None);

        begin("KJV".into(), "NIV", None);
        assert_eq!(on_send("John", 3), None, "the verse cited with the request");
        assert_eq!(on_send("John", 3), None);
        assert_eq!(on_send("Psalms", 23), Some("KJV".into()));

        clear();
        begin("ESV".into(), "NIV", Some(("Isaiah".into(), 7)));
        begin("NIV".into(), "NLT", Some(("Isaiah".into(), 7)));
        assert_eq!(on_send("Mark", 1), Some("ESV".into()), "asked twice: back to the church's own");

        begin("KJV".into(), "NLT", Some(("Isaiah".into(), 7)));
        clear();
        assert_eq!(on_send("John", 3), None, "the operator changed it");
    }
}
