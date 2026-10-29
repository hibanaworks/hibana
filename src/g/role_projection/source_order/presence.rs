//! Route conditions from the typed source, shared by all role checks.
use crate::g::{ProgramSourceNode, SourceArm, SourceContext, SourceScope, SourceSelection};

#[derive(Clone, Copy)]
struct Condition {
    selection: SourceSelection,
    parent: u16,
}

#[cfg(all(test, hibana_repo_tests))]
mod tests {
    use super::*;
    use crate::g::{self, ProgramShape};

    #[test]
    fn cached_presence_matches_all_completions_of_each_partial_choice() {
        type Step = g::Send<0, 1, g::Msg<1, ()>>;
        type Steps = g::Seq<
            g::Route<g::Seq<Step, g::Route<Step, Step>>, g::Par<g::Route<Step, Step>, Step>>,
            g::Roll<Step>,
        >;
        let presence = SourcePresence::<32>::new(&Steps::SOURCE_NODE);
        let scopes = [
            SourceScope {
                start: 0,
                split: 3,
                stop: 6,
            },
            SourceScope {
                start: 1,
                split: 2,
                stop: 3,
            },
            SourceScope {
                start: 3,
                split: 4,
                stop: 5,
            },
        ];
        for partial in 0..27 {
            let mut choices = [0; 3];
            let mut remainder = partial;
            let mut selected = std::vec::Vec::new();
            for (position, scope) in scopes.iter().enumerate() {
                choices[position] = remainder % 3;
                remainder /= 3;
                if choices[position] != 0 {
                    selected.push(SourceSelection {
                        scope: *scope,
                        arm: if choices[position] == 1 {
                            SourceArm::Left
                        } else {
                            SourceArm::Right
                        },
                    });
                }
            }
            let path = SourceContext::new(&selected);
            for event in 0..7 {
                let mut completions = 0;
                let mut present = 0;
                for full in 0..8 {
                    let arms = [full & 1 == 0, full & 2 == 0, full & 4 == 0];
                    if choices
                        .iter()
                        .zip(arms)
                        .any(|(&choice, left)| choice != 0 && (choice == 1) != left)
                    {
                        continue;
                    }
                    completions += 1;
                    let occurs = match event {
                        0 => arms[0],
                        1 => arms[0] && arms[1],
                        2 => arms[0] && !arms[1],
                        3 => !arms[0] && arms[2],
                        4 => !arms[0] && !arms[2],
                        5 => !arms[0],
                        6 => true,
                        _ => unreachable!(),
                    };
                    present += usize::from(occurs);
                }
                match presence.active(event, &path) {
                    Ok(true) => assert_eq!(present, completions),
                    Ok(false) => assert_eq!(present, 0),
                    Err(scope) => {
                        assert!(present > 0 && present < completions);
                        assert!(path.get(scope).is_none());
                    }
                }
            }
        }
    }

    #[test]
    fn source_rows_and_merged_paths_match_exhaustive_choice_assignments() {
        type Step = g::Send<0, 1, g::Msg<1, ()>>;
        type Steps = g::Seq<
            g::Route<g::Seq<Step, g::Route<Step, Step>>, g::Par<g::Route<Step, Step>, Step>>,
            g::Roll<Step>,
        >;
        let source = &Steps::SOURCE_NODE;
        let presence = SourcePresence::<32>::new(source);
        let scopes = [
            SourceScope {
                start: 0,
                split: 3,
                stop: 6,
            },
            SourceScope {
                start: 1,
                split: 2,
                stop: 3,
            },
            SourceScope {
                start: 3,
                split: 4,
                stop: 5,
            },
        ];
        let occurs = |event, arms: [bool; 3]| match event {
            0 => arms[0],
            1 => arms[0] && arms[1],
            2 => arms[0] && !arms[1],
            3 => !arms[0] && arms[2],
            4 => !arms[0] && !arms[2],
            5 => !arms[0],
            6 => true,
            _ => unreachable!(),
        };
        let mut indexed = [None; 32];
        for first in 0..source.event_count() {
            let mut successors = [false; 32];
            super::super::mark_source_successors(source, 0, first, &mut successors);
            for (second, &expected) in successors.iter().enumerate().take(source.event_count()) {
                assert_eq!(expected, source.before(first, second));
                assert!(presence.merge_path(first, &mut indexed));
                let compatible = presence.merge_path(second, &mut indexed);
                let mut common = 0;
                for full in 0..8 {
                    let arms = [full & 1 == 0, full & 2 == 0, full & 4 == 0];
                    let selected = core::array::from_fn::<_, 3, _>(|i| SourceSelection {
                        scope: scopes[i],
                        arm: if arms[i] {
                            SourceArm::Left
                        } else {
                            SourceArm::Right
                        },
                    });
                    let path = SourceContext::new(&selected);
                    let matches = indexed
                        .iter()
                        .flatten()
                        .all(|choice| path.get(choice.scope) == Some(choice.arm));
                    if occurs(first, arms) && occurs(second, arms) {
                        common += 1;
                        assert!(matches);
                    }
                    if compatible {
                        assert_eq!(matches, occurs(first, arms) && occurs(second, arms));
                    }
                }
                assert_eq!(compatible, common != 0);
                if compatible {
                    let path = SourceContext::from_index(&indexed);
                    assert!(matches!(presence.active(first, &path), Ok(true)));
                    assert!(matches!(presence.active(second, &path), Ok(true)));
                    for choice in indexed.iter().flatten() {
                        assert_eq!(path.get(choice.scope), Some(choice.arm));
                    }
                    let different_scope = SourceScope {
                        start: 1,
                        split: 3,
                        stop: 6,
                    };
                    assert!(path.get(different_scope).is_none());
                    let extra = path.select(SourceSelection {
                        scope: different_scope,
                        arm: SourceArm::Right,
                    });
                    assert!(matches!(extra.get(different_scope), Some(SourceArm::Right)));
                }
                presence.clear_path(first, &mut indexed);
                presence.clear_path(second, &mut indexed);
                assert!(indexed.iter().all(Option::is_none));
            }
        }
    }
}

pub(super) struct SourcePresence<const E: usize> {
    conditions: [Option<Condition>; E],
    heads: [u16; E],
}

impl<const E: usize> SourcePresence<E> {
    pub(super) const fn new(source: &ProgramSourceNode) -> Self {
        let mut index = Self {
            conditions: [None; E],
            heads: [u16::MAX; E],
        };
        let mut next = 0;
        index.collect(source, 0, u16::MAX, &mut next);
        index
    }

    const fn collect(
        &mut self,
        source: &ProgramSourceNode,
        start: usize,
        parent: u16,
        next: &mut usize,
    ) {
        match source {
            ProgramSourceNode::Send(_) => self.heads[start] = parent,
            ProgramSourceNode::Seq(left, right, _) | ProgramSourceNode::Par(left, right, _) => {
                self.collect(left, start, parent, next);
                self.collect(right, start + left.event_count(), parent, next);
            }
            ProgramSourceNode::Roll(body, _) => self.collect(body, start, parent, next),
            ProgramSourceNode::Route { left, right, .. } => {
                let split = start + left.event_count();
                let scope = SourceScope {
                    start: start as u16,
                    split: split as u16,
                    stop: (split + right.event_count()) as u16,
                };
                // Each route already needs four marker rows in E. Its two
                // condition links fit the same source-derived capacity.
                if *next + 2 > E || *next + 2 >= u16::MAX as usize {
                    crate::invariant();
                }
                let first = *next;
                *next += 2;
                self.conditions[first] = Some(Condition {
                    selection: SourceSelection {
                        scope,
                        arm: SourceArm::Left,
                    },
                    parent,
                });
                self.conditions[first + 1] = Some(Condition {
                    selection: SourceSelection {
                        scope,
                        arm: SourceArm::Right,
                    },
                    parent,
                });
                self.collect(left, start, first as u16, next);
                self.collect(right, split, (first + 1) as u16, next);
            }
        }
    }

    /// Merge an event's required choices into compiler scratch. Binary source
    /// splits are unique; compare the full scope before sharing an entry.
    pub(super) const fn merge_path(
        &self,
        event: usize,
        selections: &mut [Option<SourceSelection>; E],
    ) -> bool {
        let mut link = self.heads[event];
        while link != u16::MAX {
            let Some(condition) = self.conditions[link as usize] else {
                crate::invariant();
            };
            let slot = condition.selection.scope.split as usize;
            if let Some(previous) = selections[slot] {
                if previous.scope.start != condition.selection.scope.start
                    || previous.scope.stop != condition.selection.scope.stop
                {
                    crate::invariant();
                }
                if previous.arm as u8 != condition.selection.arm as u8 {
                    return false;
                }
            }
            selections[slot] = Some(condition.selection);
            link = condition.parent;
        }
        true
    }

    pub(super) const fn clear_path(
        &self,
        event: usize,
        selections: &mut [Option<SourceSelection>; E],
    ) {
        let mut link = self.heads[event];
        while link != u16::MAX {
            let Some(condition) = self.conditions[link as usize] else {
                crate::invariant();
            };
            selections[condition.selection.scope.split as usize] = None;
            link = condition.parent;
        }
    }

    pub(super) const fn active(
        &self,
        event: usize,
        path: &SourceContext<'_>,
    ) -> Result<bool, SourceScope> {
        let mut link = self.heads[event];
        let mut missing = None;
        while link != u16::MAX {
            let Some(condition) = self.conditions[link as usize] else {
                crate::invariant();
            };
            match path.get(condition.selection.scope) {
                None => missing = Some(condition.selection.scope),
                Some(arm) if arm as u8 == condition.selection.arm as u8 => (),
                Some(_) => return Ok(false),
            }
            link = condition.parent;
        }
        match missing {
            Some(scope) => Err(scope),
            None => Ok(true),
        }
    }
}

#[cfg(all(test, hibana_repo_tests))]
mod word_tests {
    use super::super::{LaneBits, RowBits, pair_candidates};

    #[test]
    fn all_word_interval_boundaries_match_individual_row_membership() {
        for start in 0..=128 {
            for end in start..=128 {
                let mut bits = RowBits::<2>::empty();
                bits.insert_range(start, end);
                for row in 0..128 {
                    assert_eq!(bits.get(row), (start..end).contains(&row));
                }
                bits.clear(2);
                assert_eq!(bits.words, [0; 2]);
            }
        }
    }

    #[test]
    fn lane_masks_cover_all_u8_lanes_including_partial_final_words() {
        let mut masks = LaneBits::<1>::empty();
        for lane in 0..256 {
            masks.insert(lane, lane % 64);
        }
        for lane in 0..256 {
            assert_eq!(masks.get(lane), 1 << (lane % 64));
        }
    }

    #[test]
    fn the_word_filter_checks_missing_and_unwanted_ordering() {
        for expected in [0, 1, u64::MAX, 0xaaaaaaaaaaaaaaaa] {
            for actual in [0, 1, u64::MAX, 0x5555555555555555] {
                assert_eq!(
                    pair_candidates(expected, actual, actual, 0),
                    expected ^ actual
                );
                assert_eq!(pair_candidates(expected, actual, actual, u64::MAX), 0);
            }
        }
    }
}
