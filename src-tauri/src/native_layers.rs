//! Output Control's switches (Scripture, Slides and songs, Media, Timer) for the senders that draw
//! the words themselves: NDI and Spout, Syphon before a captured frame arrives, and DeckLink where
//! output windows can't be captured (Windows). The sender keeps them and applies them every time it
//! draws, so a switch that is off stays off whatever the page sends or when it sends it.

use serde::Deserialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Layers {
    pub bible: bool,
    pub slide: bool,
    pub media: bool,
    pub timer: bool,
}

impl Default for Layers {
    fn default() -> Self {
        Layers { bible: true, slide: true, media: true, timer: true }
    }
}

impl Layers {
    /// The verse and reference to draw: none when the switch for that kind of words is off.
    pub fn words<'a>(&self, bible: bool, verse: &'a str, reference: &'a str) -> (&'a str, &'a str) {
        if (bible && self.bible) || (!bible && self.slide) { (verse, reference) } else { ("", "") }
    }
    pub fn media<'a, T>(&self, media: Option<&'a T>) -> Option<&'a T> {
        if self.media { media } else { None }
    }
    pub fn timer<'a>(&self, text: &'a str) -> &'a str {
        if self.timer { text } else { "" }
    }
}

#[cfg(test)]
mod tests {
    use super::Layers;

    #[test]
    fn scripture_switched_off_hides_only_scripture() {
        let l = Layers { bible: false, ..Layers::default() };
        assert_eq!(l.words(true, "For God so loved", "John 3:16"), ("", ""));
        assert_eq!(l.words(false, "Amazing grace", "Song"), ("Amazing grace", "Song"));
    }

    #[test]
    fn slides_switched_off_keep_scripture() {
        let l = Layers { slide: false, ..Layers::default() };
        assert_eq!(l.words(false, "Amazing grace", "Song"), ("", ""));
        assert_eq!(l.words(true, "Jesus wept", "John 11:35"), ("Jesus wept", "John 11:35"));
    }

    #[test]
    fn media_and_timer_follow_their_switches() {
        let l = Layers { media: false, timer: false, ..Layers::default() };
        assert_eq!(l.media(Some(&1)), None);
        assert_eq!(l.timer("5:00"), "");
        assert_eq!(Layers::default().timer("5:00"), "5:00");
    }

    #[test]
    fn a_missing_switch_from_the_page_counts_as_on() {
        let l: Layers = serde_json::from_str(r#"{"bible":false}"#).unwrap();
        assert_eq!(l, Layers { bible: false, ..Layers::default() });
    }
}
