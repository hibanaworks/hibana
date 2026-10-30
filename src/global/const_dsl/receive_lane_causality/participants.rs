use super::{CausalRoles, EffList};
use crate::g::SourceActorImage;

/// Construction-only interval unions of the possible physical participants.
/// The leaves occupy `len..2*len`; node zero is unused. Two nodes per arena
/// row cover the complete source event domain without a separate capacity.
pub(crate) struct Participants<const E: usize> {
    nodes: [[CausalRoles; 2]; E],
    len: usize,
}

impl<const E: usize> Participants<E> {
    pub(crate) const fn empty(len: usize) -> Self {
        if len > E {
            crate::invariant();
        }
        Self {
            nodes: [[CausalRoles::empty(); 2]; E],
            len,
        }
    }

    pub(super) const fn new<const D: usize>(
        events: &EffList<E>,
        actors: Option<&SourceActorImage<E, D>>,
    ) -> Self {
        let mut result = Self::empty(events.len());
        let mut event = 0;
        while event < result.len {
            let mut members = CausalRoles::empty();
            match actors {
                Some(image) => {
                    let bytes = image.possible_actors(event);
                    let mut word = 0;
                    while word < members.0.len() {
                        let byte = word * 8;
                        members.0[word] = u64::from_le_bytes([
                            bytes[byte],
                            bytes[byte + 1],
                            bytes[byte + 2],
                            bytes[byte + 3],
                            bytes[byte + 4],
                            bytes[byte + 5],
                            bytes[byte + 6],
                            bytes[byte + 7],
                        ]);
                        word += 1;
                    }
                }
                None => {
                    let atom = events.atom_at(event);
                    members.insert(atom.from);
                    members.insert(atom.to);
                }
            }
            result.set(result.len + event, members);
            event += 1;
        }
        result.build_parents();
        result
    }

    pub(crate) const fn for_lane(&mut self, events: &EffList<E>, source: &Self, lane: u8) {
        if events.len() != source.len || self.len != source.len {
            crate::invariant();
        }
        let mut event = 0;
        while event < self.len {
            let members = if events.atom_at(event).lane == lane {
                source.at(event)
            } else {
                CausalRoles::empty()
            };
            self.set(self.len + event, members);
            event += 1;
        }
        self.build_parents();
    }

    const fn build_parents(&mut self) {
        let mut node = self.len;
        while node > 1 {
            node -= 1;
            self.set(node, self.get(node * 2).union(self.get(node * 2 + 1)));
        }
    }

    const fn get(&self, node: usize) -> CausalRoles {
        self.nodes[node / 2][node % 2]
    }

    const fn set(&mut self, node: usize, members: CausalRoles) {
        self.nodes[node / 2][node % 2] = members;
    }

    pub(crate) const fn at(&self, event: usize) -> CausalRoles {
        if event >= self.len {
            crate::invariant();
        }
        self.get(self.len + event)
    }

    pub(crate) const fn members(&self, start: usize, stop: usize) -> CausalRoles {
        if start > stop || stop > self.len {
            crate::invariant();
        }
        let mut result = CausalRoles::empty();
        let mut left = self.len + start;
        let mut right = self.len + stop;
        while left < right {
            if !left.is_multiple_of(2) {
                result = result.union(self.get(left));
                left += 1;
            }
            if !right.is_multiple_of(2) {
                right -= 1;
                result = result.union(self.get(right));
            }
            left /= 2;
            right /= 2;
        }
        result
    }

    pub(super) const fn covered(&self, start: usize, stop: usize, facts: &CausalRoles) -> bool {
        if start > stop || stop > self.len {
            crate::invariant();
        }
        let mut left = self.len + start;
        let mut right = self.len + stop;
        while left < right {
            if !left.is_multiple_of(2) {
                if !self.get(left).is_subset_of(facts) {
                    return false;
                }
                left += 1;
            }
            if !right.is_multiple_of(2) {
                right -= 1;
                if !self.get(right).is_subset_of(facts) {
                    return false;
                }
            }
            left /= 2;
            right /= 2;
        }
        true
    }
}

#[cfg(all(test, hibana_repo_tests))]
mod tests {
    use super::*;
    use crate::{
        eff::{EffAtom, EventOrigin, EventPayload},
        g::{SourceArm, SourceScope, SourceSelection},
    };

    #[test]
    fn interval_unions_match_every_selected_physical_participant() {
        let roles = [0, 63, 64, 127, 128, 191, 192, 255];
        let first = SourceScope {
            start: 0,
            split: 1,
            stop: 2,
        };
        let second = SourceScope {
            start: 2,
            split: 3,
            stop: 4,
        };
        let mut checks = 0;
        for len in 0..=9 {
            let mut events = EffList::<16>::new();
            let mut references = [[0u16; 2]; 9];
            for (event, pair) in references.iter_mut().enumerate().take(len) {
                events.push_event_mut(EffAtom {
                    from: roles[event % roles.len()],
                    to: roles[(event + 1) % roles.len()],
                    label: 0,
                    payload: EventPayload::Wire(0),
                    origin: EventOrigin::User,
                    lane: (event % 3) as u8,
                });
                *pair = [
                    if event % 2 == 0 {
                        256
                    } else {
                        roles[event % 8] as u16
                    },
                    257,
                ];
            }
            let image = SourceActorImage::<16, 2>::causal_fixture(
                &references[..len],
                &[(first, [63, 128]), (second, [256, 255])],
            );
            for actors in [None, Some(&image)] {
                let source = Participants::new(&events, actors);
                let mut filtered = Participants::empty(events.len());
                for lane in [None, Some(0), Some(1), Some(2), Some(0)] {
                    let index = match lane {
                        None => &source,
                        Some(lane) => {
                            filtered.for_lane(&events, &source, lane);
                            &filtered
                        }
                    };
                    for encoding in 0..256u16 {
                        let mut facts = CausalRoles::empty();
                        for (bit, role) in roles.iter().enumerate() {
                            if encoding & (1 << bit) != 0 {
                                facts.insert(*role);
                            }
                        }
                        for start in 0..=len {
                            for stop in start..=len {
                                let mut expected = true;
                                for event in start..stop {
                                    if lane.is_some_and(|lane| events.atom_at(event).lane != lane) {
                                        continue;
                                    }
                                    if let Some(image) = actors {
                                        for left in [SourceArm::Left, SourceArm::Right] {
                                            for right in [SourceArm::Left, SourceArm::Right] {
                                                let (sender, receiver) = image
                                                    .actors_at(
                                                        event,
                                                        &[
                                                            SourceSelection {
                                                                scope: first,
                                                                arm: left,
                                                            },
                                                            SourceSelection {
                                                                scope: second,
                                                                arm: right,
                                                            },
                                                        ],
                                                    )
                                                    .unwrap();
                                                expected &= facts.contains(sender)
                                                    && facts.contains(receiver);
                                            }
                                        }
                                    } else {
                                        let atom = events.atom_at(event);
                                        expected &=
                                            facts.contains(atom.from) && facts.contains(atom.to);
                                    }
                                }
                                assert_eq!(
                                    index.covered(start, stop, &facts),
                                    expected,
                                    "len={len} encoding={encoding} range={start}..{stop} owned={}",
                                    actors.is_some()
                                );
                                assert_eq!(
                                    index.members(start, stop).is_subset_of(&facts),
                                    expected
                                );
                                if start + 1 == stop {
                                    assert_eq!(index.at(start).0, index.members(start, stop).0);
                                }
                                checks += 1;
                            }
                        }
                    }
                }
            }
        }
        assert_eq!(checks, 563_200);
    }
}
