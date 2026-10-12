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
    // run[a][b]: equal words starting at verse[a], speech[b].
    let w = t + 1;
    let mut run = vec![0u16; (v + 1) * w];
    for a in (0..v).rev() {
        for b in (0..t).rev() {
            if speech[b] == Some(verse[a]) {
                run[a * w + b] = 1 + run[(a + 1) * w + b + 1];
            }
        }
    }
    let mut best = Run { start: 0, len: 0 };
    for a in 0..v {
        for b in 0..t {
            let len = (run[a * w + b] as usize).min(cap);
            if len > best.len {
                best = Run { start: a, len };
            }
        }
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
