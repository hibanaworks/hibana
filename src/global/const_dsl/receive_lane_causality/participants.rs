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
    const fn empty(len: usize) -> Self {
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

    /// Remove exactly the colors used by a physically intersecting event in
    /// this interval. Immutable participant unions prune disjoint subtrees;
    /// current colors are read only at relevant leaves, after recoloring.
    pub(crate) const fn exclude_colors(
        &self,
        events: &EffList<E>,
        (start, stop, members): (usize, usize, CausalRoles),
        colors: &mut [u8],
    ) {
        if start > stop || stop > self.len || events.len() != self.len {
            crate::invariant();
        }
        let mut left = self.len + start;
        let mut right = self.len + stop;
        while left < right {
            if !left.is_multiple_of(2) {
                self.exclude_node(events, left, members, colors);
                left += 1;
            }
            if !right.is_multiple_of(2) {
                right -= 1;
                self.exclude_node(events, right, members, colors);
            }
            left /= 2;
            right /= 2;
        }
    }

    const fn exclude_node(
        &self,
        events: &EffList<E>,
        node: usize,
        members: CausalRoles,
        colors: &mut [u8],
    ) {
        if self.get(node).is_disjoint(members) {
            return;
        }
        if node >= self.len {
            let lane = events.atom_at(node - self.len).lane as usize;
            if lane / 8 < colors.len() {
                colors[lane / 8] &= !(1 << (lane % 8));
            }
        } else {
            self.exclude_node(events, node * 2, members, colors);
            self.exclude_node(events, node * 2 + 1, members, colors);
        }
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
    fn color_exclusion_reads_current_colors_across_the_complete_wire_domain() {
        let mut events = EffList::<3>::new();
        for role in [0, 127, 255] {
            events.push_event_mut(EffAtom {
                from: role,
                to: role,
                label: 0,
                payload: EventPayload::Wire(0),
                origin: EventOrigin::User,
                lane: 0,
            });
        }
        let source = Participants::new::<0>(&events, None);
        for position in 0..3 {
            let mut query = CausalRoles::empty();
            query.insert(events.atom_at(position).from);
            for color in 0..=u8::MAX {
                let mut row = events.atom_at(position);
                row.lane = color;
                events.replace_atom(position, row);
                for width in [1, 2, 16, 32] {
                    let mut colors = [u8::MAX; 32];
                    source.exclude_colors(&events, (0, 3, query), &mut colors[..width]);
                    for observed in 0..width * 8 {
                        assert_eq!(
                            colors[observed / 8] & (1 << (observed % 8)) != 0,
                            observed != color as usize
                        );
                    }
                }
            }
        }
    }

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
                for lane in [None, Some(0), Some(1), Some(2), Some(0)] {
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
                                if let Some(lane) = lane {
                                    let outside = CausalRoles([
                                        !facts.0[0],
                                        !facts.0[1],
                                        !facts.0[2],
                                        !facts.0[3],
                                    ]);
                                    let mut colors = [u8::MAX; 32];
                                    source.exclude_colors(
                                        &events,
                                        (start, stop, outside),
                                        &mut colors,
                                    );
                                    assert_eq!(
                                        colors[lane as usize / 8] & (1 << (lane % 8)) != 0,
                                        expected,
                                        "len={len} encoding={encoding} range={start}..{stop} lane={lane} owned={}",
                                        actors.is_some()
                                    );
                                } else {
                                    assert_eq!(source.covered(start, stop, &facts), expected);
                                    assert_eq!(
                                        source.members(start, stop).is_subset_of(&facts),
                                        expected
                                    );
                                    if start + 1 == stop {
                                        assert_eq!(
                                            source.at(start).0,
                                            source.members(start, stop).0
                                        );
                                    }
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
