use core::ops::Range;

use super::EventCursor;

/// A read-only view of the existing completion authority. Completed steps never
/// require descriptor decoding; incomplete steps are still checked individually.
struct PendingEvents<'a> {
    words: &'a [u32],
    word: usize,
    end: usize,
    pending: u32,
}

fn prefix_mask(len: usize) -> u32 {
    if len >= u32::BITS as usize {
        u32::MAX
    } else {
        (1u32 << len) - 1
    }
}

impl<'a> PendingEvents<'a> {
    fn new(words: &'a [u32], range: Range<usize>) -> Self {
        let word = range.start / u32::BITS as usize;
        let pending = if range.is_empty() {
            0
        } else {
            !words[word]
                & (u32::MAX << (range.start % u32::BITS as usize))
                & prefix_mask(range.end - word * u32::BITS as usize)
        };
        Self {
            words,
            word,
            end: range.end,
            pending,
        }
    }
}

impl Iterator for PendingEvents<'_> {
    type Item = usize;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.pending != 0 {
                let bit = self.pending.trailing_zeros() as usize;
                self.pending &= self.pending - 1;
                return Some(self.word * u32::BITS as usize + bit);
            }
            let start = (self.word + 1) * u32::BITS as usize;
            if start >= self.end {
                return None;
            }
            self.word += 1;
            self.pending = !self.words[self.word] & prefix_mask(self.end - start);
        }
    }
}

impl EventCursor {
    pub(super) fn pending_event_steps(
        &self,
        range: Range<usize>,
    ) -> impl Iterator<Item = usize> + '_ {
        if range.start > range.end || range.end > self.local_steps_len() {
            crate::invariant();
        }
        PendingEvents::new(self.completed_event_words(), range)
    }
}

#[cfg(all(test, hibana_repo_tests))]
mod tests {
    use super::*;

    #[test]
    fn bitmap_scan_matches_each_step_across_word_edges_and_empty_ranges() {
        for completed in [0, u32::MAX, 0xaaaaaaaa, 0x55555555, 0x80000001] {
            let words = [completed, completed.rotate_left(7), !completed, completed];
            for start in 0..=128 {
                for end in start..=128 {
                    let actual: std::vec::Vec<_> = PendingEvents::new(&words, start..end).collect();
                    let expected: std::vec::Vec<_> = (start..end)
                        .filter(|step| words[step / 32] & (1 << (step % 32)) == 0)
                        .collect();
                    assert_eq!(actual, expected, "{completed:08x} {start}..{end}");
                }
            }
        }
    }

    #[test]
    fn compact_step_domain_scans_the_last_partial_word_and_reentry_clear() {
        let end = usize::from(u16::MAX);
        let mut words = std::vec![u32::MAX; end.div_ceil(32)];
        assert_eq!(PendingEvents::new(&words, 0..end).next(), None);
        for step in [0, 31, 32, end - 1] {
            words[step / 32] &= !(1 << (step % 32));
        }
        assert_eq!(
            PendingEvents::new(&words, 0..end).collect::<std::vec::Vec<_>>(),
            [0, 31, 32, end - 1]
        );
        assert_eq!(PendingEvents::new(&words, end..end).next(), None);
    }
}
