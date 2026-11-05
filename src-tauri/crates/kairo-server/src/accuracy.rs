//! The percentage the operator sees on a detected verse: how often a match of that kind was the
//! verse being cited or read, measured on recorded sermons. It is not the engine's own score,
//! which still decides what is sent, offered or hidden (its thresholds are tuned on it).
//!
//! Counts from `replay --by-score` (kairo-eval) on 2026-11-05: the 8 recorded sermons with
//! Deepgram transcripts plus the 2 with offline Core ML transcripts. A screen send is right unless
//! the replay counts it wrong; a Possible Match is right when it is the verse cited or read within
//! a minute. A kind seen only a few times is pulled toward the average of its target (a weighted
//! prior of `PRIOR_WEIGHT` matches), so five right out of five shows about 98%, not 100%.
//! Re-measure and update the counts when detection changes.

/// (method, right, seen) for verses sent to the screen.
const SCREEN: &[(&str, u32, u32)] = &[
    ("direct", 272, 280),
    ("stream", 167, 168),
    ("direct-partial", 18, 18),
    ("verbatim", 11, 11),
    ("chapter-keyword", 9, 9),
    ("fingerprint", 5, 5),
    ("context-citation", 4, 4),
    ("continuation", 1, 2),
];

/// (method, right, seen) for Possible Matches shown to the operator.
const OFFERED: &[(&str, u32, u32)] = &[
    ("stream", 46, 61),
    ("paraphrase", 22, 37),
    ("verbatim", 22, 25),
    ("direct", 4, 8),
    ("chapter-keyword", 3, 3),
    ("fingerprint", 0, 1),
];

const PRIOR_WEIGHT: f32 = 20.0;

/// How often a match of `method` sent to the screen (`screen`) or offered was right, 0–1.
pub fn measured(method: &str, screen: bool) -> f32 {
    let table = if screen { SCREEN } else { OFFERED };
    let (right, seen) = table.iter().fold((0, 0), |(r, n), x| (r + x.1, n + x.2));
    let average = right as f32 / seen as f32;
    let (r, n) = table.iter().find(|x| x.0 == method).map_or((0, 0), |x| (x.1, x.2));
    (r as f32 + PRIOR_WEIGHT * average) / (n as f32 + PRIOR_WEIGHT)
}

/// Adds the measured accuracy (`accuracy`, 0–1) to each detected verse sent to the UI.
pub fn mark(verses: &mut [serde_json::Value], method: &str, screen: bool) {
    let a = measured(method, screen);
    for v in verses {
        v["accuracy"] = serde_json::json!(a);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pct(m: &str, screen: bool) -> u32 {
        (measured(m, screen) * 100.0).round() as u32
    }

    #[test]
    fn screen_sends_show_how_often_their_kind_was_right() {
        assert_eq!(pct("direct", true), 97);
        assert_eq!(pct("stream", true), 99);
        // Few examples: near the screen average (98%), never a perfect 100%.
        assert_eq!(pct("fingerprint", true), 98);
        assert!(pct("continuation", true) < pct("direct", true));
    }

    #[test]
    fn possible_matches_are_lower_and_kinds_differ() {
        assert_eq!(pct("verbatim", false), 81);
        assert_eq!(pct("stream", false), 75);
        assert_eq!(pct("paraphrase", false), 64);
        assert!(pct("verbatim", false) < pct("verbatim", true));
    }

    #[test]
    fn a_kind_never_measured_shows_its_targets_average() {
        assert_eq!(pct("semantic", true), 98);
        assert_eq!(pct("semantic", false), 72);
    }

    #[test]
    fn mark_adds_accuracy_and_keeps_the_score() {
        let mut v = vec![serde_json::json!({ "reference": "John 3:16", "similarity": 1.0 })];
        mark(&mut v, "direct", true);
        assert_eq!(v[0]["similarity"], 1.0);
        assert!((v[0]["accuracy"].as_f64().unwrap() - 0.9714).abs() < 0.001);
    }
}
