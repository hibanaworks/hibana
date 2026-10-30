use crate::{eff::EffAtom, global::const_dsl::EffList};

const ABSENT: u16 = u16::MAX;

#[derive(Clone, Copy)]
pub(super) struct Direction {
    key: u16,
    sender: u8,
    latest: u16,
    other: u16,
}

impl Direction {
    pub(super) const EMPTY: Self = Self {
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
pub(super) struct ReceiveDirectionIndex<'a> {
    directions: &'a mut [Direction],
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

impl<'a> ReceiveDirectionIndex<'a> {
    pub(super) const fn has_changes(&self) -> bool {
        let mut direction = 0;
        while direction < self.len {
            if self.directions[direction].other != ABSENT {
                return true;
            }
            direction += 1;
        }
        false
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

    pub(super) const fn new<const E: usize>(
        events: &EffList<E>,
        storage: &'a mut [Direction],
    ) -> Self {
        let end = events.len();
        if end > u16::MAX as usize || storage.len() < end {
            crate::invariant();
        }
        let mut index = Self {
            directions: storage,
            len: 0,
            end,
        };
        let mut keys = [0u16; E];
        let mut positions = [0u16; E];
        let mut scratch = [0u16; E];
        let mut count = 0;
        let mut event = 0;
        while event < end {
            let atom = events.atom_at(event);
            if atom.from != atom.to {
                keys[event] = (atom.to as u16) << 8 | atom.lane as u16;
                positions[count] = event as u16;
                count += 1;
            }
            event += 1;
        }
        sort_keys(&keys, &mut positions, &mut scratch, count);
        let mut position = 0;
        while position < count {
            let event = positions[position];
            let atom = events.atom_at(event as usize);
            let key = keys[event as usize];
            if index.len != 0 && index.directions[index.len - 1].key == key {
                let previous = index.directions[index.len - 1];
                index.directions[index.len] = Direction {
                    key,
                    sender: atom.from,
                    latest: event,
                    other: if previous.sender == atom.from {
                        previous.other
                    } else {
                        previous.latest
                    },
                };
            } else {
                index.directions[index.len] = Direction {
                    key,
                    sender: atom.from,
                    latest: event,
                    other: ABSENT,
                };
            }
            index.len += 1;
            position += 1;
        }
        index
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

pub(crate) const fn has_receive_sender_changes<const E: usize>(events: &EffList<E>) -> bool {
    let mut storage = [Direction::EMPTY; E];
    ReceiveDirectionIndex::new(events, &mut storage).has_changes()
}
