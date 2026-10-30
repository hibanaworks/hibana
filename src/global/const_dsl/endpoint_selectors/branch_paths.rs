//! Projection-only branch ancestry, built once instead of rescanning markers
//! for every pair of endpoint operations. No field is stored in the runtime.
use super::super::{
    ScopeKind, ScopeMarkerView,
    scope_ranges::{parallel_arm_ranges_from_enter, route_arm_ranges_from_first_enter},
};

const NONE: u16 = u16::MAX;

#[derive(Clone, Copy)]
struct Fork {
    parent: u16,
    split: usize,
    kind: ScopeKind,
}

pub(super) struct BranchPaths<const E: usize> {
    event_forks: [u16; E],
    forks: [Fork; E],
    fork_count: usize,
}

impl<const E: usize> BranchPaths<E> {
    pub(super) const fn new(markers: ScopeMarkerView<'_>) -> Self {
        let mut result = Self {
            event_forks: [NONE; E],
            forks: [Fork {
                parent: NONE,
                split: 0,
                kind: ScopeKind::Route,
            }; E],
            fork_count: 0,
        };
        let mut fork_count = 0;
        let mut idx = 0;
        while idx < markers.len() {
            let marker = markers.at(idx);
            if marker.event.is_primary_enter() {
                let ranges = match marker.scope_id.kind() {
                    Some(ScopeKind::Route) => Some((
                        ScopeKind::Route,
                        route_arm_ranges_from_first_enter(markers, idx),
                    )),
                    Some(ScopeKind::Parallel) => match parallel_arm_ranges_from_enter(markers, idx)
                    {
                        Some((a, b, c, d)) => Some((ScopeKind::Parallel, [(a, b), (c, d)])),
                        None => panic!("parallel scope requires two nonempty arms"),
                    },
                    _ => None,
                };
                if let Some((kind, [(start, split), (_, end)])) = ranges {
                    if fork_count >= E || fork_count >= NONE as usize || start >= E || end > E {
                        panic!("branch ancestry exceeds the source arena");
                    }
                    // Source marker ordering visits enclosing enters before
                    // contained enters, including equal-range wrappers.
                    result.forks[fork_count] = Fork {
                        parent: result.event_forks[start],
                        split,
                        kind,
                    };
                    let mut event = start;
                    while event < end {
                        result.event_forks[event] = fork_count as u16;
                        event += 1;
                    }
                    fork_count += 1;
                }
            }
            idx += 1;
        }
        result.fork_count = fork_count;
        result
    }

    /// The lowest common fork separates alternatives or independent work.
    /// If only one event is inside a nested fork, it is a sequential prefix.
    pub(super) const fn separation(&self, left: usize, right: usize) -> Option<ScopeKind> {
        let mut a = self.event_forks[left];
        let mut b = self.event_forks[right];
        while a != b {
            if a == NONE || (b != NONE && a < b) {
                b = self.forks[b as usize].parent;
            } else {
                a = self.forks[a as usize].parent;
            }
        }
        if a == NONE {
            return None;
        }
        let fork = self.forks[a as usize];
        if (left < fork.split) != (right < fork.split) {
            Some(fork.kind)
        } else {
            None
        }
    }

    /// A sequential predecessor must occur on every path containing `later`.
    pub(super) const fn precedes_on_every_path(&self, earlier: usize, later: usize) -> bool {
        if earlier >= later || self.separation(earlier, later).is_some() {
            return false;
        }
        let mut fork = self.event_forks[earlier];
        while fork != NONE {
            let ancestor = self.forks[fork as usize];
            if matches!(ancestor.kind, ScopeKind::Route) {
                let mut other = self.event_forks[later];
                while other != NONE && other != fork {
                    other = self.forks[other as usize].parent;
                }
                if other == NONE || (earlier < ancestor.split) != (later < ancestor.split) {
                    return false;
                }
            }
            fork = ancestor.parent;
        }
        true
    }

    pub(super) const fn record_guard(&self, arms: &mut [u8; E], earlier: usize) {
        let fork = self.event_forks[earlier];
        assert!(fork != NONE);
        arms[fork as usize] |= if earlier < self.forks[fork as usize].split {
            1
        } else {
            2
        };
    }

    /// An earlier choice guards a later operation when both alternatives do.
    /// Inside a fork only the candidate's own arm may supply its predecessor.
    pub(super) const fn guards_every_path(&self, arms: &mut [u8; E], candidate: usize) -> bool {
        let mut index = self.fork_count;
        while index > 0 {
            index -= 1;
            let fork = self.forks[index];
            let mut ancestor = self.event_forks[candidate];
            while ancestor != NONE && ancestor as usize != index {
                ancestor = self.forks[ancestor as usize].parent;
            }
            let covered = if ancestor != NONE {
                arms[index] & if candidate < fork.split { 1 } else { 2 } != 0
            } else if matches!(fork.kind, ScopeKind::Route) {
                arms[index] == 3
            } else {
                arms[index] != 0
            };
            if covered {
                if fork.parent == NONE {
                    return true;
                }
                let parent = fork.parent as usize;
                arms[parent] |= if fork.split < self.forks[parent].split {
                    1
                } else {
                    2
                };
            }
        }
        false
    }
}

#[cfg(all(test, hibana_repo_tests))]
mod tests {
    use super::*;
    use crate::{
        eff::{EffAtom, EventOrigin},
        global::const_dsl::{EffList, ReentryMark, ScopeId},
    };

    #[test]
    fn nested_alternatives_and_parallel_paths_keep_distinct_meanings() {
        let mut source = EffList::<11>::new_partitioned(4, 7, 0);
        for label in 0..4 {
            source.push_event_mut(EffAtom {
                from: 0,
                to: 1,
                label,
                payload: crate::eff::EventPayload::Wire(0),
                origin: EventOrigin::User,
                lane: 0,
            });
        }
        source.push_parallel_scope_mut(ScopeId::new(ScopeKind::Parallel, 1), 0, 1, 2);
        source.push_route_scope_mut(
            ScopeId::new(ScopeKind::Route, 0),
            0,
            2,
            4,
            ReentryMark::SinglePass,
        );
        let paths = BranchPaths::<11>::new(source.scope_markers());
        for left in 0..4 {
            for right in 0..4 {
                let expected = if (left < 2) != (right < 2) {
                    Some(ScopeKind::Route)
                } else if left < 2 && left != right {
                    Some(ScopeKind::Parallel)
                } else {
                    None
                };
                assert_eq!(paths.separation(left, right), expected);
            }
        }
    }

    #[test]
    fn an_optional_prefix_cannot_guard_a_head_after_the_join() {
        let mut source = EffList::<16>::new_partitioned(5, 11, 0);
        for label in 0..5 {
            source.push_event_mut(EffAtom {
                from: 0,
                to: 1,
                label,
                payload: crate::eff::EventPayload::Wire(0),
                origin: EventOrigin::User,
                lane: 0,
            });
        }
        source.push_parallel_scope_mut(ScopeId::new(ScopeKind::Parallel, 1), 0, 1, 2);
        source.push_route_scope_mut(
            ScopeId::new(ScopeKind::Route, 0),
            0,
            3,
            4,
            ReentryMark::SinglePass,
        );
        let paths = BranchPaths::<16>::new(source.scope_markers());
        assert!(!paths.precedes_on_every_path(0, 1));
        assert!(paths.precedes_on_every_path(0, 2));
        assert!(paths.precedes_on_every_path(1, 2));
        for optional in 0..4 {
            assert!(!paths.precedes_on_every_path(optional, 4));
        }
        for present in 0u8..16 {
            let mut guards = [0; 16];
            for event in 0..4 {
                if present & (1 << event) != 0 {
                    paths.record_guard(&mut guards, event);
                }
            }
            assert_eq!(
                paths.guards_every_path(&mut guards, 4),
                present & 7 != 0 && present & 8 != 0,
            );
        }
    }
}
