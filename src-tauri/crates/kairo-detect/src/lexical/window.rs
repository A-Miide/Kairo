//! The longest run of a verse's words that the speech contains, in order.

use super::lexicon::WordId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Run {
    /// Position in the verse.
    pub start: usize,
    pub len: usize,
}

/// The longest stretch of consecutive verse words found consecutively in the
/// speech — at least `min` words, counted up to `cap`. The earliest verse
/// position wins a tie.
pub fn longest_run(verse: &[WordId], speech: &[Option<WordId>], min: usize, cap: usize) -> Option<Run> {
    let (v, t) = (verse.len(), speech.len());
    if v == 0 || t == 0 {
        return None;
    }
    // Runs ending at verse[a], speech[b], one row of the verse at a time; a
    // run's start is fixed along it, so the longest (capped) run with the
    // earliest start is kept.
    let mut prev = vec![0u16; t + 1];
    let mut cur = vec![0u16; t + 1];
    let mut best = Run { start: 0, len: 0 };
    for a in 0..v {
        for b in 0..t {
            if speech[b] == Some(verse[a]) {
                let l = prev[b] + 1;
                cur[b + 1] = l;
                let (len, start) = ((l as usize).min(cap), a + 1 - l as usize);
                if len > best.len || (len == best.len && start < best.start) {
                    best = Run { start, len };
                }
            } else {
                cur[b + 1] = 0;
            }
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    (best.len >= min.max(1)).then_some(best)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[u32]) -> Vec<Option<u32>> {
        v.iter().map(|x| Some(*x)).collect()
    }

    #[test]
    fn finds_the_longest_in_order_run() {
        let verse = [1, 2, 3, 4, 5, 6, 7];
        assert_eq!(longest_run(&verse, &s(&[9, 2, 3, 4, 9, 5, 6]), 2, 99), Some(Run { start: 1, len: 3 }));
        assert_eq!(longest_run(&verse, &s(&[1, 2, 3, 4, 5, 6, 7]), 2, 5), Some(Run { start: 0, len: 5 }));
        assert_eq!(longest_run(&verse, &s(&[7, 6, 5]), 2, 99), None);
        assert_eq!(longest_run(&verse, &[Some(1), None, Some(3)], 2, 99), None);
    }
}
