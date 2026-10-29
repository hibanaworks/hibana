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

/// Source-only lookup for the last receive from a different sender. Each
/// direction needs its latest event and its latest event from another sender.
/// Entries are sorted by logical receiver/lane; no actor ownership is stored.
pub(super) struct ReceiveDirectionIndex<'a> {
    directions: &'a mut [Direction],
    len: usize,
    start: usize,
    end: usize,
}

impl<'a> ReceiveDirectionIndex<'a> {
    const fn position(&self, key: u16) -> usize {
        let mut low = 0;
        let mut high = self.len;
        while low < high {
            let middle = low + (high - low) / 2;
            if self.directions[middle].key < key {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        low
    }

    pub(super) const fn new<const E: usize>(
        events: &EffList<E>,
        range: (usize, usize),
        storage: &'a mut [Direction],
    ) -> Self {
        let (start, end) = range;
        if start > end
            || end > events.len()
            || events.len() > u16::MAX as usize
            || storage.len() < end - start
        {
            crate::invariant();
        }
        let mut index = Self {
            directions: storage,
            len: 0,
            start,
            end,
        };
        let mut event = end;
        while event > start {
            event -= 1;
            let atom = events.atom_at(event);
            if atom.from != atom.to {
                let key = (atom.to as u16) << 8 | atom.lane as u16;
                let position = index.position(key);
                if position < index.len && index.directions[position].key == key {
                    let direction = &mut index.directions[position];
                    if direction.other == ABSENT && direction.sender != atom.from {
                        direction.other = event as u16;
                    }
                } else {
                    let mut move_to = index.len;
                    while move_to > position {
                        index.directions[move_to] = index.directions[move_to - 1];
                        move_to -= 1;
                    }
                    index.directions[position] = Direction {
                        key,
                        sender: atom.from,
                        latest: event as u16,
                        other: ABSENT,
                    };
                    index.len += 1;
                }
            }
        }
        index
    }

    pub(super) const fn last_change(&self, earlier: EffAtom, start: usize) -> Option<usize> {
        if start < self.start || start > self.end {
            crate::invariant();
        }
        let key = (earlier.to as u16) << 8 | earlier.lane as u16;
        let position = self.position(key);
        if position == self.len || self.directions[position].key != key {
            return None;
        }
        let direction = &self.directions[position];
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
