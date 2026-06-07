//! First-recv dispatch cache for offer materialization.

use crate::global::typestate::{FirstRecvDispatchSpec, MAX_FIRST_RECV_DISPATCH, StateIndex};

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel) struct FirstRecvDispatchEntry {
    arm: u8,
    target: StateIndex,
}

impl FirstRecvDispatchEntry {
    const EMPTY: Self = Self {
        arm: 0,
        target: StateIndex::MAX,
    };

    #[inline]
    const fn from_spec(entry: FirstRecvDispatchSpec) -> Self {
        Self {
            arm: entry.arm(),
            target: entry.target(),
        }
    }

    #[inline]
    fn contributes_to_arm(self) -> bool {
        self.arm < 2 && !self.target.is_max()
    }
}

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel) struct FirstRecvDispatchCache {
    entries: [FirstRecvDispatchEntry; MAX_FIRST_RECV_DISPATCH],
    len: u8,
    arm_mask: u8,
}

impl FirstRecvDispatchCache {
    pub(in crate::endpoint::kernel) const EMPTY: Self = Self {
        entries: [FirstRecvDispatchEntry::EMPTY; MAX_FIRST_RECV_DISPATCH],
        len: 0,
        arm_mask: 0,
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
            }
            idx += 1;
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel) const fn arm_has_dispatch(&self, arm: u8) -> bool {
        arm < 2 && (self.arm_mask & (1u8 << arm)) != 0
    }
}
