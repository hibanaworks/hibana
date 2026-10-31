//! Causal queries for fixed physical actor bindings, batched across receives.
//! Each bit is one received source event. Parallel arms share their input;
//! choices join conditionally on the occurrence of that bit's source event.
use super::{CausalRoles, EffList, FlowIndex, FlowRange, ScopeKind};
use crate::g::SourceActorImage;

#[derive(Clone, Copy)]
pub(crate) struct ReceiveOrder<'a> {
    words: &'a [u64],
    repeated: &'a [u64],
    stride: usize,
    seeds: &'a [u16],
    closures: &'a [Option<RollRange>],
}

impl ReceiveOrder<'_> {
    pub(super) const fn contains(self, before: usize, after: usize) -> bool {
        let seed = self.seeds[before] as usize;
        seed == u16::MAX as usize
            || self.words[after * self.stride + seed / 64] & (1 << (seed % 64)) != 0
    }

    pub(super) const fn covers_roll(self, event: usize, start: usize, end: usize) -> bool {
        matches!(self.closures[event], Some(row) if row.start as usize == start && row.end as usize == end)
    }

    pub(super) const fn repeated_contains(self, before: usize, after: usize) -> bool {
        let seed = self.seeds[before] as usize;
        seed == u16::MAX as usize
            || self.repeated[after * self.stride + seed / 64] & (1 << (seed % 64)) != 0
    }
}

/// Receives that can have a different logical sender at the same receiver.
/// Other receives create no receive-order obligation, regardless of color.
pub(crate) struct ReceiveSeeds<const E: usize> {
    ordinals: [u16; E],
    prefixes: [u16; E],
    len: usize,
    pub(crate) count: usize,
}

impl<const E: usize> ReceiveSeeds<E> {
    pub(crate) const fn new(events: &EffList<E>) -> Self {
        let mut first = [u16::MAX; 256];
        let mut changing = CausalRoles::empty();
        let mut event = 0;
        while event < events.len() {
            let atom = events.atom_at(event);
            if atom.from != atom.to {
                let previous = &mut first[atom.to as usize];
                if *previous == u16::MAX {
                    *previous = atom.from as u16;
                } else if *previous != atom.from as u16 {
                    changing.insert(atom.to);
                }
            }
            event += 1;
        }
        let mut result = Self {
            ordinals: [u16::MAX; E],
            prefixes: [0; E],
            len: events.len(),
            count: 0,
        };
        event = 0;
        while event < events.len() {
            result.prefixes[event] = result.count as u16;
            let atom = events.atom_at(event);
            if atom.from != atom.to && changing.contains(atom.to) {
                result.ordinals[event] = result.count as u16;
                result.count += 1;
            }
            event += 1;
        }
        result
    }

    const fn before(&self, event: usize) -> usize {
        if event == self.len {
            self.count
        } else {
            self.prefixes[event] as usize
        }
    }
}

#[derive(Clone, Copy)]
struct RollRange {
    start: u16,
    end: u16,
}

pub(crate) struct ReceiveRelations<const E: usize, const W: usize> {
    rows: [[u64; W]; E],
    repeated: [[u64; W]; E],
    closures: [Option<RollRange>; E],
    seeds: [u16; E],
}

impl<const E: usize, const W: usize> ReceiveRelations<E, W> {
    pub(crate) const fn new<const D: usize>(
        events: &EffList<E>,
        scopes: &FlowIndex<E>,
        actors: Option<&SourceActorImage<E, D>>,
        seeds: &ReceiveSeeds<E>,
    ) -> Self {
        if seeds.count.div_ceil(64) > W
            || matches!(actors, Some(image) if image.decision_count() != 0)
        {
            crate::invariant();
        }
        let mut builder = Builder {
            events,
            scopes,
            actors,
            active: [0; 256],
            active_len: 0,
            words: seeds.count.div_ceil(64),
            seeds,
        };
        let mut seen = CausalRoles::empty();
        let mut event = 0;
        while event < events.len() {
            let (sender, receiver) = builder.pair(event);
            let pair = [sender, receiver];
            let mut side = 0;
            while side < 2 {
                let actor = pair[side];
                if !seen.contains(actor) {
                    seen.insert(actor);
                    builder.active[builder.active_len] = actor;
                    builder.active_len += 1;
                }
                side += 1;
            }
            event += 1;
        }
        let mut result = Self {
            rows: [[0; W]; E],
            repeated: [[0; W]; E],
            closures: [None; E],
            seeds: seeds.ordinals,
        };
        builder.visit(
            FlowRange {
                start: 0,
                end: events.len(),
                marker_floor: 0,
            },
            &mut [[0; W]; 256],
            &mut result,
        );
        result
    }

    pub(crate) const fn as_ref(&self) -> ReceiveOrder<'_> {
        ReceiveOrder {
            words: self.rows.as_flattened(),
            repeated: self.repeated.as_flattened(),
            stride: W,
            seeds: &self.seeds,
            closures: &self.closures,
        }
    }
}

struct Builder<'a, const E: usize, const D: usize> {
    events: &'a EffList<E>,
    scopes: &'a FlowIndex<E>,
    actors: Option<&'a SourceActorImage<E, D>>,
    active: [u8; 256],
    active_len: usize,
    words: usize,
    seeds: &'a ReceiveSeeds<E>,
}

/// The bits of one source interval that fall in this machine word.
const fn interval(start: usize, end: usize, word: usize) -> u64 {
    let base = word * 64;
    if end <= base || start >= base + 64 {
        return 0;
    }
    let low = start.saturating_sub(base);
    let high = if end < base + 64 { end - base } else { 64 };
    (u64::MAX << low) & (u64::MAX >> (64 - high))
}

#[cfg(kani)]
mod kani {
    #[kani::proof]
    #[kani::unwind(5)]
    fn receive_seed_ordinals_cover_exactly_the_changing_receivers() {
        use crate::eff::{EffAtom, EventOrigin, EventPayload};
        let mut events = super::EffList::<4>::new();
        for _ in 0..4 {
            events.push_event_mut(EffAtom {
                from: kani::any(),
                to: kani::any(),
                lane: kani::any(),
                label: 0,
                payload: EventPayload::Wire(0),
                origin: EventOrigin::User,
            });
        }
        let seeds = super::ReceiveSeeds::new(&events);
        let mut count = 0;
        for first in 0..4 {
            let a = events.atom_at(first);
            let mut expected = false;
            for second in 0..4 {
                let b = events.atom_at(second);
                expected |= a.from != a.to && b.from != b.to && a.to == b.to && a.from != b.from;
            }
            assert_eq!(seeds.before(first), count);
            assert_eq!(
                seeds.ordinals[first],
                if expected { count as u16 } else { u16::MAX }
            );
            if expected {
                count += 1;
            }
        }
        assert_eq!(seeds.before(4), count);
        assert_eq!(seeds.count, count);
        kani::cover!(count == 0);
        kani::cover!(count == 4);
    }

    #[kani::proof]
    fn interval_mask_is_exact_for_the_compact_source_domain() {
        let first = usize::from(kani::any::<u16>());
        let second = usize::from(kani::any::<u16>());
        let start = first.min(second);
        let end = first.max(second);
        let word = usize::from(kani::any::<u16>()) / 64;
        let bit = usize::from(kani::any::<u8>()) % 64;
        let point = word * 64 + bit;
        assert_eq!(
            super::interval(start, end, word) & (1u64 << bit) != 0,
            start <= point && point < end
        );
        kani::cover!(start == end);
        kani::cover!(start == 0 && end == u16::MAX as usize);
        kani::cover!(word == 1023 && bit == 63);
    }
}

impl<const E: usize, const D: usize> Builder<'_, E, D> {
    /// Fresh-visit routes are independent of the seed's first-visit route.
    /// No receive in this pass creates another copy of that source seed.
    const fn next_visit<const W: usize>(
        &self,
        mut range: FlowRange,
        facts: &mut [[u64; W]; 256],
        rows: &mut [[u64; W]; E],
        body: RollRange,
    ) {
        let first_word = self.seeds.before(body.start as usize) / 64;
        let last_word = self.seeds.before(body.end as usize).div_ceil(64);
        while range.start < range.end {
            if let Some((marker, kind, split, end)) = self.scopes.scope_at(&mut range) {
                let left = FlowRange {
                    start: range.start,
                    end: split,
                    marker_floor: marker + 1,
                };
                if matches!(kind, ScopeKind::Roll) {
                    self.next_visit(left, facts, rows, body);
                } else {
                    let mut right = [[0; W]; 256];
                    let mut word = first_word;
                    while word < last_word {
                        let mut member = 0;
                        while member < self.active_len {
                            let actor = self.active[member] as usize;
                            right[actor][word] = facts[actor][word];
                            member += 1;
                        }
                        word += 1;
                    }
                    self.next_visit(left, facts, rows, body);
                    self.next_visit(
                        FlowRange {
                            start: split,
                            end,
                            marker_floor: marker + 1,
                        },
                        &mut right,
                        rows,
                        body,
                    );
                    word = first_word;
                    while word < last_word {
                        let mut member = 0;
                        while member < self.active_len {
                            let actor = self.active[member] as usize;
                            let a = facts[actor][word];
                            let b = right[actor][word];
                            facts[actor][word] = if matches!(kind, ScopeKind::Parallel) {
                                a | b
                            } else {
                                a & b
                            };
                            member += 1;
                        }
                        word += 1;
                    }
                }
                range.start = end;
            } else {
                let (sender, receiver) = self.pair(range.start);
                let mut word = first_word;
                while word < last_word {
                    let known = facts[sender as usize][word];
                    rows[range.start][word] = known;
                    facts[receiver as usize][word] |= known;
                    word += 1;
                }
                range.start += 1;
            }
        }
    }

    const fn pair(&self, event: usize) -> (u8, u8) {
        match self.actors {
            None => {
                let atom = self.events.atom_at(event);
                (atom.from, atom.to)
            }
            Some(image) => {
                let pair = image.event_actors()[event];
                if pair.sender.0 >= 256 || pair.receiver.0 >= 256 {
                    crate::invariant();
                }
                (pair.sender.0 as u8, pair.receiver.0 as u8)
            }
        }
    }

    const fn visit<const W: usize>(
        &self,
        mut range: FlowRange,
        facts: &mut [[u64; W]; 256],
        result: &mut ReceiveRelations<E, W>,
    ) {
        while range.start < range.end {
            if let Some((marker, kind, split, end)) = self.scopes.scope_at(&mut range) {
                let left = FlowRange {
                    start: range.start,
                    end: split,
                    marker_floor: marker + 1,
                };
                if matches!(kind, ScopeKind::Roll) {
                    self.visit(left, facts, result);
                    let body = RollRange {
                        start: range.start as u16,
                        end: end as u16,
                    };
                    let mut next = [[0; W]; 256];
                    let first_seed = self.seeds.before(range.start);
                    let last_seed = self.seeds.before(end);
                    let mut word = first_seed / 64;
                    while word < last_seed.div_ceil(64) {
                        let mask = interval(first_seed, last_seed, word);
                        let mut member = 0;
                        while member < self.active_len {
                            let actor = self.active[member] as usize;
                            next[actor][word] = facts[actor][word] & mask;
                            member += 1;
                        }
                        word += 1;
                    }
                    self.next_visit(left, &mut next, &mut result.repeated, body);
                    let mut event = range.start;
                    while event < end {
                        // Postorder keeps the outermost roll for these rows.
                        result.closures[event] = Some(body);
                        event += 1;
                    }
                } else {
                    let mut right = [[0; W]; 256];
                    let seed_start = self.seeds.before(range.start);
                    let seed_split = self.seeds.before(split);
                    let seed_end = self.seeds.before(end);
                    let mut word = 0;
                    while word < self.words {
                        let (left_mask, right_mask) = if matches!(kind, ScopeKind::Route) {
                            (
                                interval(seed_start, seed_split, word),
                                interval(seed_split, seed_end, word),
                            )
                        } else {
                            (0, 0)
                        };
                        let mut member = 0;
                        while member < self.active_len {
                            let actor = self.active[member] as usize;
                            right[actor][word] = facts[actor][word];
                            // An event in the other arm cannot coexist with
                            // this arm's send: its implication is true.
                            facts[actor][word] |= right_mask;
                            right[actor][word] |= left_mask;
                            member += 1;
                        }
                        word += 1;
                    }
                    self.visit(left, facts, result);
                    self.visit(
                        FlowRange {
                            start: split,
                            end,
                            marker_floor: marker + 1,
                        },
                        &mut right,
                        result,
                    );
                    word = 0;
                    while word < self.words {
                        let left_mask = interval(seed_start, seed_split, word);
                        let right_mask = interval(seed_split, seed_end, word);
                        let mut member = 0;
                        while member < self.active_len {
                            let actor = self.active[member] as usize;
                            let a = facts[actor][word];
                            let b = right[actor][word];
                            facts[actor][word] = if matches!(kind, ScopeKind::Parallel) {
                                a | b
                            } else {
                                (a & b) | (a & left_mask) | (b & right_mask)
                            };
                            member += 1;
                        }
                        word += 1;
                    }
                }
                range.start = end;
            } else {
                let (sender, receiver) = self.pair(range.start);
                let mut word = 0;
                while word < self.words {
                    let known = facts[sender as usize][word];
                    result.rows[range.start][word] = known;
                    facts[receiver as usize][word] |= known;
                    word += 1;
                }
                let seed = self.seeds.ordinals[range.start] as usize;
                if seed != u16::MAX as usize {
                    facts[receiver as usize][seed / 64] |= 1 << (seed % 64);
                }
                range.start += 1;
            }
        }
    }
}
