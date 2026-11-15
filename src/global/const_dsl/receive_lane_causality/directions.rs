use crate::{eff::EffAtom, global::const_dsl::EffList};

const ABSENT: u16 = u16::MAX;

#[derive(Clone, Copy)]
pub(crate) struct Direction {
    key: u16,
    sender: u8,
    latest: u16,
    other: u16,
}

impl Direction {
    pub(crate) const EMPTY: Self = Self {
        key: 0,
        sender: 0,
        latest: ABSENT,
        other: ABSENT,
    };
}

/// Source-only lookup for the last receive from a different sender in any
/// interval. Rows are sorted by logical receiver/lane and source position.
/// Each row links to its direction's latest different sender. Nested scopes
/// query this one index without rebuilding their receive summaries.
pub(crate) struct ReceiveDirectionIndex<'a> {
    directions: &'a mut [Direction],
    homogeneous: &'a mut [bool],
    receivers: [(u16, u16); 256],
    len: usize,
    end: usize,
}

/// Source-ordered events for each logical sender/receiver pair. Recoloring
/// changes neither the pairs nor their order, so one index serves every lane
/// proposal during allocation.
pub(crate) struct LogicalDirections<const E: usize> {
    head: [u16; E],
    next: [u16; E],
}

impl<const E: usize> LogicalDirections<E> {
    pub(crate) const fn new(events: &EffList<E>) -> Self {
        let mut keys = [0u16; E];
        let mut positions = [0u16; E];
        let mut scratch = [0u16; E];
        let mut event = 0;
        while event < events.len() {
            let atom = events.atom_at(event);
            keys[event] = (atom.from as u16) << 8 | atom.to as u16;
            positions[event] = event as u16;
            event += 1;
        }
        sort_keys(&keys, &mut positions, &mut scratch, events.len());
        let mut result = Self {
            head: [ABSENT; E],
            next: [ABSENT; E],
        };
        let mut head = ABSENT;
        let mut row = 0;
        while row < events.len() {
            let event = positions[row];
            if row == 0 || keys[positions[row - 1] as usize] != keys[event as usize] {
                head = event;
            } else {
                result.next[positions[row - 1] as usize] = event;
            }
            result.head[event as usize] = head;
            row += 1;
        }
        result
    }

    pub(crate) const fn first(&self, event: usize) -> u16 {
        self.head[event]
    }

    pub(crate) const fn next(&self, event: u16) -> u16 {
        self.next[event as usize]
    }
}

/// Read-only receiver/color chains for one unmodified lane-assignment cut.
/// At most one key per source event can exist. Empty buckets and bounded probes
/// cover full occupancy without a capacity beyond the source arena.
pub(crate) struct ReceiveLanes<const E: usize> {
    keys: [u16; E],
    first: [u16; E],
    next: [u16; E],
}

impl<const E: usize> ReceiveLanes<E> {
    const fn slot(key: u16) -> usize {
        (key ^ key.rotate_right(7)) as usize % E
    }

    pub(crate) const fn new(events: &EffList<E>) -> Self {
        let mut result = Self {
            keys: [0; E],
            first: [ABSENT; E],
            next: [ABSENT; E],
        };
        let mut event = events.len();
        while event != 0 {
            event -= 1;
            let atom = events.atom_at(event);
            if atom.from != atom.to {
                let key = (atom.to as u16) << 8 | atom.lane as u16;
                let mut slot = Self::slot(key);
                while result.first[slot] != ABSENT && result.keys[slot] != key {
                    slot = if slot + 1 == E { 0 } else { slot + 1 };
                }
                result.keys[slot] = key;
                result.next[event] = result.first[slot];
                result.first[slot] = event as u16;
            }
        }
        result
    }

    pub(crate) const fn first(&self, receiver: u8, lane: u8) -> u16 {
        if E == 0 {
            return ABSENT;
        }
        let key = (receiver as u16) << 8 | lane as u16;
        let mut slot = Self::slot(key);
        let mut inspected = 0;
        while inspected < E {
            if self.first[slot] == ABSENT {
                return ABSENT;
            }
            if self.keys[slot] == key {
                return self.first[slot];
            }
            slot = if slot + 1 == E { 0 } else { slot + 1 };
            inspected += 1;
        }
        ABSENT
    }

    pub(crate) const fn next(&self, event: u16) -> u16 {
        self.next[event as usize]
    }
}

impl<'a> ReceiveDirectionIndex<'a> {
    pub(crate) const fn has_changes(&self) -> bool {
        let mut direction = 0;
        while direction < self.len {
            if self.directions[direction].other != ABSENT {
                return true;
            }
            direction += 1;
        }
        false
    }
    /// Actual source rows whose entire receiver/color group has one sender.
    /// Such a row cannot start a sender-change obligation in any subrange or
    /// repeated visit; no range lookup is needed for it.
    pub(super) const fn homogeneous_rows(&self) -> &[bool] {
        self.homogeneous.split_at(self.end).0
    }

    const fn position(&self, key: u16, end: usize) -> usize {
        let mut low = 0;
        let mut high = self.len;
        while low < high {
            let middle = low + (high - low) / 2;
            let row = self.directions[middle];
            if row.key < key || (row.key == key && (row.latest as usize) < end) {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        low
    }

    pub(crate) const fn new<const E: usize>(
        events: &EffList<E>,
        storage: &'a mut [Direction],
        homogeneous: &'a mut [bool],
    ) -> Self {
        let end = events.len();
        if end > u16::MAX as usize || storage.len() < end || homogeneous.len() < end {
            crate::invariant();
        }
        let mut index = Self {
            directions: storage,
            homogeneous,
            receivers: [(0, 0); 256],
            len: 0,
            end,
        };
        // Receiver chains are already in source order. Stable counting over
        // only occupied lane buckets creates the same (receiver,lane,event)
        // order without repeatedly radix-sorting the complete event arena.
        let mut first = [ABSENT; 256];
        let mut next = [ABSENT; E];
        let mut receivers = super::CausalRoles::empty();
        let mut event = end;
        while event != 0 {
            event -= 1;
            let atom = events.atom_at(event);
            if atom.from != atom.to {
                next[event] = first[atom.to as usize];
                first[atom.to as usize] = event as u16;
                receivers.insert(atom.to);
            }
        }
        while let Some(receiver) = receivers.pop_first() {
            let start = index.len;
            index.len =
                index.populate_receiver(events, receiver, (first[receiver as usize], &next), start);
            index.receivers[receiver as usize] = (start as u16, index.len as u16);
        }
        index
    }

    const fn populate_receiver<const E: usize>(
        &mut self,
        events: &EffList<E>,
        receiver: u8,
        (first, next): (u16, &[u16]),
        start: usize,
    ) -> usize {
        let mut offsets = [0u16; 256];
        let mut colors = super::CausalRoles::empty();
        let mut event = first;
        while event != ABSENT {
            let lane = events.atom_at(event as usize).lane;
            offsets[lane as usize] += 1;
            colors.insert(lane);
            event = next[event as usize];
        }
        let mut end = start;
        while let Some(color) = colors.pop_first() {
            let count = offsets[color as usize];
            offsets[color as usize] = end as u16;
            end += count as usize;
        }
        let starts = offsets;
        event = first;
        while event != ABSENT {
            let atom = events.atom_at(event as usize);
            let lane = atom.lane as usize;
            let position = offsets[lane] as usize;
            let other = if offsets[lane] != starts[lane] {
                let previous = self.directions[position - 1];
                if previous.sender == atom.from {
                    previous.other
                } else {
                    previous.latest
                }
            } else {
                ABSENT
            };
            self.directions[position] = Direction {
                key: (receiver as u16) << 8 | atom.lane as u16,
                sender: atom.from,
                latest: event,
                other,
            };
            offsets[lane] += 1;
            event = next[event as usize];
        }
        event = first;
        while event != ABSENT {
            let lane = events.atom_at(event as usize).lane as usize;
            self.homogeneous[event as usize] =
                self.directions[offsets[lane] as usize - 1].other == ABSENT;
            event = next[event as usize];
        }
        end
    }

    /// Only lane colors changed. Logical receiver groups and their source
    /// order retain the same storage intervals throughout allocation.
    pub(crate) const fn refresh<const E: usize>(
        &mut self,
        events: &EffList<E>,
        scopes: &super::FlowIndex<E>,
        mut changed: super::CausalRoles,
    ) {
        while let Some(receiver) = changed.pop_first() {
            let (start, end) = self.receivers[receiver as usize];
            let actual = self.populate_receiver(
                events,
                receiver,
                (scopes.first_receive_to(receiver), &scopes.next_receive),
                start as usize,
            );
            if actual != end as usize {
                crate::invariant();
            }
        }
    }

    pub(super) const fn last_change(
        &self,
        earlier: EffAtom,
        range: (usize, usize),
    ) -> Option<usize> {
        let (start, end) = range;
        if start > end || end > self.end {
            crate::invariant();
        }
        let key = (earlier.to as u16) << 8 | earlier.lane as u16;
        let position = self.position(key, end);
        if position == 0 || self.directions[position - 1].key != key {
            return None;
        }
        let direction = &self.directions[position - 1];
        let event = if direction.sender == earlier.from {
            direction.other
        } else {
            direction.latest
        };
        if event == ABSENT || (event as usize) < start {
            None
        } else {
            Some(event as usize + 1)
        }
    }
}

// Small sources need fewer histogram buckets. Every pass retains ties in
// source order; all 16 key bits are sorted for every source size.
const fn sort_keys<const E: usize>(
    keys: &[u16; E],
    positions: &mut [u16; E],
    scratch: &mut [u16; E],
    count: usize,
) {
    if E <= 4 {
        sort_positions::<E, 4>(keys, positions, scratch, count);
    } else if E <= 64 {
        sort_positions::<E, 16>(keys, positions, scratch, count);
    } else {
        sort_positions::<E, 256>(keys, positions, scratch, count);
    }
}

const fn sort_positions<const E: usize, const B: usize>(
    keys: &[u16; E],
    positions: &mut [u16; E],
    scratch: &mut [u16; E],
    count: usize,
) {
    let bits = match B {
        4 => 2,
        16 => 4,
        256 => 8,
        _ => crate::invariant(),
    };
    let mut shift = 0;
    while shift < 16 {
        radix_pass::<E, B>(keys, positions, scratch, count, shift);
        shift += bits;
        radix_pass::<E, B>(keys, scratch, positions, count, shift);
        shift += bits;
    }
}

const fn radix_pass<const E: usize, const B: usize>(
    keys: &[u16; E],
    input: &[u16; E],
    output: &mut [u16; E],
    len: usize,
    shift: u8,
) {
    let mut offsets = [0u16; B];
    let mut high = 0;
    let mut row = 0;
    while row < len {
        let key = ((keys[input[row] as usize] >> shift) & (B - 1) as u16) as usize;
        offsets[key] += 1;
        if high < key {
            high = key;
        }
        row += 1;
    }
    let mut total = 0;
    let mut key = 0;
    while key <= high {
        let count = offsets[key];
        offsets[key] = total;
        total += count;
        key += 1;
    }
    row = 0;
    while row < len {
        let key = ((keys[input[row] as usize] >> shift) & (B - 1) as u16) as usize;
        let target = &mut offsets[key];
        output[*target as usize] = input[row];
        *target += 1;
        row += 1;
    }
}

#[cfg(all(test, hibana_repo_tests))]
mod tests {
    use super::*;
    use crate::eff::{EventOrigin, EventPayload};
    fn event(receiver: u8, lane: u8) -> EffAtom {
        EffAtom {
            from: receiver ^ 1,
            to: receiver,
            lane,
            label: 0,
            origin: EventOrigin::User,
            payload: EventPayload::Wire(0),
        }
    }
    #[test]
    fn receive_color_chains_preserve_every_wire_key_and_source_order() {
        for key in 0..=u16::MAX {
            let receiver = (key >> 8) as u8;
            let lane = key as u8;
            let mut local = event(receiver, lane);
            local.from = receiver;
            let events = EffList::<8>::new()
                .push(event(receiver, lane))
                .push(local)
                .push(event(receiver, lane.wrapping_add(1)))
                .push(event(receiver, lane));
            let index = ReceiveLanes::new(&events);
            assert_eq!(index.first(receiver, lane), 0);
            assert_eq!(index.next(0), 3);
            assert_eq!(index.next(3), ABSENT);
            assert_eq!(index.first(receiver, lane.wrapping_add(1)), 2);
            assert_eq!(index.next(2), ABSENT);
            assert_eq!(index.first(receiver ^ 64, lane), ABSENT);
        }
    }
    #[test]
    fn full_collision_tables_terminate_on_absent_keys() {
        let mut events = EffList::<8>::new();
        let mut absent = None;
        for key in 0..=u16::MAX {
            if ReceiveLanes::<8>::slot(key) == 0 {
                if events.len() < 8 {
                    events.push_event_mut(event((key >> 8) as u8, key as u8));
                } else {
                    absent = Some(key);
                    break;
                }
            }
        }
        assert_eq!(events.len(), 8);
        let index = ReceiveLanes::new(&events);
        for position in 0..8 {
            let atom = events.atom_at(position);
            assert_eq!(index.first(atom.to, atom.lane), position as u16);
            assert_eq!(index.next(position as u16), ABSENT);
        }
        let absent = absent.unwrap();
        assert_eq!(index.first((absent >> 8) as u8, absent as u8), ABSENT);
        let empty = ReceiveLanes::new(&EffList::<0>::new());
        assert_eq!(empty.first(0, 0), ABSENT);
    }
}
