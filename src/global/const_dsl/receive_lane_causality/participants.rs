use super::{CausalRoles, EffList};
use crate::g::SourceActorImage;

/// Construction-only interval unions of the possible physical participants.
/// The leaves occupy `len..2*len`; node zero is unused. Two nodes per arena
/// row cover the complete source event domain without a separate capacity.
pub(super) struct Participants<const E: usize> {
    nodes: [[CausalRoles; 2]; E],
    len: usize,
}

impl<const E: usize> Participants<E> {
    pub(super) const fn new<const D: usize>(
        events: &EffList<E>,
        actors: Option<&SourceActorImage<E, D>>,
    ) -> Self {
        let mut result = Self {
            nodes: [[CausalRoles::empty(); 2]; E],
            len: events.len(),
        };
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
        let mut node = result.len;
        while node > 1 {
            node -= 1;
            result.set(node, result.get(node * 2).union(result.get(node * 2 + 1)));
        }
        result
    }

    const fn get(&self, node: usize) -> CausalRoles {
        self.nodes[node / 2][node % 2]
    }

    const fn set(&mut self, node: usize, members: CausalRoles) {
        self.nodes[node / 2][node % 2] = members;
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
                    lane: 0,
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
                let index = Participants::new(&events, actors);
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
                                            expected &=
                                                facts.contains(sender) && facts.contains(receiver);
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
                            checks += 1;
                        }
                    }
                }
            }
        }
        assert_eq!(checks, 112_640);
    }
}
