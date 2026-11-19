//! Singing heard in the room, from a sound classifier's frame scores (YAMNet:
//! speech and music, one frame every 0.48 s). While people sing, scripture in
//! the lyrics is only offered, as with a lyrics slide up.
//!
//! Measured on eight service recordings (2026-10-19): sung worship and choir
//! scored music 0.7–0.96 with speech near 0; preaching never passed music 0.09
//! over half a minute; speaking or praying over a keyboard kept speech 0.3–0.6.
//! So singing is music high *and* speech low, held for a few seconds.

use std::collections::VecDeque;

/// Frames averaged (about 5 s).
const WINDOW: usize = 11;
const ON_MUSIC: f32 = 0.5;
const ON_SPEECH_MAX: f32 = 0.15;
const OFF_MUSIC: f32 = 0.3;
const OFF_SPEECH: f32 = 0.3;

#[derive(Default)]
pub struct MusicGate {
    frames: VecDeque<(f32, f32)>,
    singing: bool,
}

impl MusicGate {
    /// One frame's speech and music scores; returns the new state when it changes.
    pub fn push(&mut self, speech: f32, music: f32) -> Option<bool> {
        self.frames.push_back((speech, music));
        if self.frames.len() > WINDOW {
            self.frames.pop_front();
        }
        if self.frames.len() < WINDOW {
            return None;
        }
        let n = self.frames.len() as f32;
        let speech = self.frames.iter().map(|f| f.0).sum::<f32>() / n;
        let music = self.frames.iter().map(|f| f.1).sum::<f32>() / n;
        let next = if self.singing { !(music < OFF_MUSIC || speech > OFF_SPEECH) } else { music > ON_MUSIC && speech < ON_SPEECH_MAX };
        (next != self.singing).then(|| {
            self.singing = next;
            next
        })
    }

    pub fn singing(&self) -> bool {
        self.singing
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(g: &mut MusicGate, speech: f32, music: f32, n: usize) {
        for _ in 0..n {
            g.push(speech, music);
        }
    }

    #[test]
    fn singing_turns_on_and_off_with_a_few_seconds_of_evidence() {
        let mut g = MusicGate::default();
        run(&mut g, 0.6, 0.02, 30); // preaching
        assert!(!g.singing());
        run(&mut g, 0.0, 0.85, 4); // under two seconds of music
        assert!(!g.singing());
        run(&mut g, 0.0, 0.85, 20); // a song
        assert!(g.singing());
        run(&mut g, 0.6, 0.05, 20); // preaching again
        assert!(!g.singing());
    }

    #[test]
    fn talking_over_a_keyboard_is_not_singing() {
        let mut g = MusicGate::default();
        run(&mut g, 0.4, 0.7, 60);
        assert!(!g.singing());
    }
}
