//! First-recv dispatch cache for offer materialization.

use crate::global::typestate::{FirstRecvDispatchSpec, MAX_FIRST_RECV_DISPATCH, StateIndex};

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel) struct FirstRecvDispatchEntry {
    lane: u8,
    arm: u8,
    target: StateIndex,
}

impl FirstRecvDispatchEntry {
    const EMPTY: Self = Self {
        lane: 0,
        arm: 0,
        target: StateIndex::MAX,
    };

    #[inline]
    const fn from_spec(entry: FirstRecvDispatchSpec) -> Self {
        Self {
            lane: entry.lane(),
            arm: entry.arm(),
            target: entry.target(),
        }
    }

    #[inline]
    fn contributes_to_arm(self) -> bool {
        self.arm < 2 && !self.target.is_max()
    }

    #[inline]
    fn lane_bit(self) -> u8 {
        if self.lane < u8::BITS as u8 {
            1u8 << self.lane
        } else {
            0
        }
    }
}

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel) struct FirstRecvDispatchCache {
    entries: [FirstRecvDispatchEntry; MAX_FIRST_RECV_DISPATCH],
    len: u8,
    arm_mask: u8,
    lane_mask_by_arm: [u8; 2],
}

impl FirstRecvDispatchCache {
    pub(in crate::endpoint::kernel) const EMPTY: Self = Self {
        entries: [FirstRecvDispatchEntry::EMPTY; MAX_FIRST_RECV_DISPATCH],
        len: 0,
        arm_mask: 0,
        lane_mask_by_arm: [0; 2],
    };

    #[inline]
    pub(in crate::endpoint::kernel) fn record(
        &mut self,
        dispatch: [FirstRecvDispatchSpec; MAX_FIRST_RECV_DISPATCH],
        len: u8,
    ) {
        *self = Self::EMPTY;
        self.len = len;
        let mut idx = 0usize;
        while idx < len as usize {
            let entry = FirstRecvDispatchEntry::from_spec(dispatch[idx]);
            self.entries[idx] = entry;
            if entry.contributes_to_arm() {
                self.arm_mask |= 1 << entry.arm;
                self.lane_mask_by_arm[entry.arm as usize] |= entry.lane_bit();
            }
            idx += 1;
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel) const fn lane_mask_for_arm(&self, arm: u8) -> u8 {
        if arm < 2 {
            self.lane_mask_by_arm[arm as usize]
        } else {
            0
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel) const fn arm_has_dispatch(&self, arm: u8) -> bool {
        arm < 2 && (self.arm_mask & (1u8 << arm)) != 0
    }

    #[cfg(test)]
    #[inline]
    pub(in crate::endpoint::kernel) const fn len(&self) -> u8 {
        self.len
    }
}
