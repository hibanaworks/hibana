//! Mutable scope-evidence owner for endpoint kernel runtime bookkeeping.

use super::authority::RouteArmToken;
use super::evidence::ScopeEvidence;
use core::ops::{Index, IndexMut};

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum ReadyArmEvidence {
    Poll,
    Materialization,
}

impl ReadyArmEvidence {
    #[inline]
    const fn records_poll(self) -> bool {
        matches!(self, Self::Poll)
    }
}

#[derive(Clone, Copy)]
pub(super) struct ScopeEvidenceSlot {
    evidence: ScopeEvidence,
}

impl ScopeEvidenceSlot {
    const EMPTY: Self = Self {
        evidence: ScopeEvidence::EMPTY,
    };
}

#[derive(Clone, Copy)]
pub(super) struct ScopeEvidenceTable {
    slots: *mut ScopeEvidenceSlot,
    len: u16,
}

impl ScopeEvidenceTable {
    pub(super) unsafe fn init_from_parts(
        dst: *mut Self,
        slots: *mut ScopeEvidenceSlot,
        len: usize,
    ) {
        if len > u16::MAX as usize {
            crate::invariant();
        }
        /* SAFETY: endpoint initialization passes an unpublished
        `ScopeEvidenceTable` field plus its scope-evidence backing slice. This
        writes the table pointer and u16 length after the length bound check. */
        unsafe {
            core::ptr::addr_of_mut!((*dst).slots).write(slots);
            core::ptr::addr_of_mut!((*dst).len).write(len as u16);
        }
        let mut idx = 0usize;
        while idx < len {
            /* SAFETY: `idx < len` selects one slot in the unpublished
            scope-evidence backing slice; each slot is written to EMPTY before
            any endpoint evidence lookup can reach the table. */
            unsafe {
                slots.add(idx).write(ScopeEvidenceSlot::EMPTY);
            }
            idx += 1;
        }
    }

    #[inline]
    fn contains(&self, slot: usize) -> bool {
        slot < self.len as usize
    }

    #[inline]
    fn slot(&self, slot: usize) -> Option<&ScopeEvidenceSlot> {
        if !self.contains(slot) {
            return None;
        }
        /* SAFETY: `contains` bounds `slot` inside the initialized
        scope-evidence table installed during endpoint initialization. Shared
        access is tied to `&self` and only reads the selected slot. */
        Some(unsafe { &*self.slots.add(slot) })
    }

    #[inline]
    fn slot_mut(&mut self, slot: usize) -> Option<&mut ScopeEvidenceSlot> {
        if !self.contains(slot) {
            return None;
        }
        /* SAFETY: `contains` bounds `slot` inside the initialized
        scope-evidence table, and `&mut self` is the endpoint evidence mutation
        token for the selected slot. */
        Some(unsafe { &mut *self.slots.add(slot) })
    }

    #[inline]
    pub(super) fn get(&self, slot: usize) -> Option<&ScopeEvidence> {
        Some(&self.slot(slot)?.evidence)
    }

    #[inline]
    pub(super) fn get_mut(&mut self, slot: usize) -> Option<&mut ScopeEvidence> {
        Some(&mut self.slot_mut(slot)?.evidence)
    }

    pub(super) fn clear(&mut self, slot: usize) -> bool {
        let evidence = &mut self[slot];
        let changed = evidence.ack.is_some()
            || (evidence.flags & ScopeEvidence::FLAG_HAS_HINT) != 0
            || evidence.ready_arm_mask != 0
            || evidence.poll_ready_arm_mask != 0
            || evidence.flags != 0;
        if changed {
            *evidence = ScopeEvidence::EMPTY;
        }
        changed
    }
}

impl Index<usize> for ScopeEvidenceTable {
    type Output = ScopeEvidence;

    #[inline]
    fn index(&self, index: usize) -> &Self::Output {
        crate::invariant_some(self.get(index))
    }
}

impl IndexMut<usize> for ScopeEvidenceTable {
    #[inline]
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        crate::invariant_some(self.get_mut(index))
    }
}

impl ScopeEvidenceTable {
    #[inline]
    pub(super) fn record_ack(&mut self, slot: usize, token: RouteArmToken) -> bool {
        let evidence = &mut self[slot];
        let arm = token.arm().as_u8();
        if (evidence.flags & ScopeEvidence::FLAG_ACK_CONFLICT) != 0 {
            return false;
        }
        if let Some(existing) = evidence.ack
            && existing.arm().as_u8() != arm
        {
            evidence.flags |= ScopeEvidence::FLAG_ACK_CONFLICT;
            evidence.ack = None;
            evidence.ready_arm_mask = 0;
            evidence.poll_ready_arm_mask = 0;
            true
        } else if evidence.ack != Some(token) {
            evidence.ack = Some(token);
            true
        } else {
            false
        }
    }

    #[inline]
    pub(super) fn peek_ack(&self, slot: usize) -> Option<RouteArmToken> {
        let evidence = *self.get(slot)?;
        if (evidence.flags & ScopeEvidence::FLAG_ACK_CONFLICT) != 0 {
            return None;
        }
        evidence.ack
    }

    #[inline]
    pub(super) fn take_ack(&mut self, slot: usize) -> Option<RouteArmToken> {
        let evidence = self.get_mut(slot)?;
        if (evidence.flags & ScopeEvidence::FLAG_ACK_CONFLICT) != 0 {
            return None;
        }
        let token = evidence.ack;
        evidence.ack = None;
        token
    }

    #[inline]
    pub(super) fn record_frame_hint(&mut self, slot: usize, lane: u8, frame_label: u8) -> bool {
        let evidence = &mut self[slot];
        if (evidence.flags & ScopeEvidence::FLAG_HINT_CONFLICT) != 0 {
            return false;
        }
        if (evidence.flags & ScopeEvidence::FLAG_HAS_HINT) == 0 {
            evidence.hint_frame_label = frame_label;
            evidence.hint_lane = lane;
            evidence.flags |= ScopeEvidence::FLAG_HAS_HINT;
            true
        } else if evidence.hint_frame_label == frame_label && evidence.hint_lane == lane {
            false
        } else {
            evidence.flags |= ScopeEvidence::FLAG_HINT_CONFLICT;
            evidence.flags &= !ScopeEvidence::FLAG_HAS_HINT;
            evidence.hint_frame_label = 0;
            evidence.hint_lane = 0;
            true
        }
    }

    #[inline]
    pub(super) fn record_dynamic_frame_hint(
        &mut self,
        slot: usize,
        lane: u8,
        frame_label: u8,
    ) -> bool {
        let evidence = &mut self[slot];
        let captured_frame_label = evidence.hint_frame_label;
        let captured_lane = evidence.hint_lane;
        let captured_flags = evidence.flags;
        evidence.hint_frame_label = frame_label;
        evidence.hint_lane = lane;
        evidence.flags |= ScopeEvidence::FLAG_HAS_HINT;
        evidence.flags &= !ScopeEvidence::FLAG_HINT_CONFLICT;
        evidence.hint_frame_label != captured_frame_label
            || evidence.hint_lane != captured_lane
            || evidence.flags != captured_flags
    }

    #[inline]
    pub(super) fn mark_ready_arm(
        &mut self,
        slot: usize,
        arm: u8,
        evidence_kind: ReadyArmEvidence,
    ) -> bool {
        let evidence = &mut self[slot];
        let bit = ScopeEvidence::arm_bit(arm);
        let ready_changed = (evidence.ready_arm_mask & bit) == 0;
        let poll_changed =
            evidence_kind.records_poll() && (evidence.poll_ready_arm_mask & bit) == 0;
        if ready_changed {
            evidence.ready_arm_mask |= bit;
        }
        if poll_changed {
            evidence.poll_ready_arm_mask |= bit;
        }
        ready_changed || poll_changed
    }

    #[inline]
    pub(super) fn ready_arm_mask(&self, slot: usize) -> u8 {
        self[slot].ready_arm_mask
    }

    #[inline]
    pub(super) fn poll_ready_arm_mask(&self, slot: usize) -> u8 {
        self[slot].poll_ready_arm_mask
    }

    #[inline]
    pub(super) fn consume_ready_arm(&mut self, slot: usize, arm: u8) -> bool {
        let evidence = &mut self[slot];
        let bit = ScopeEvidence::arm_bit(arm);
        let ready_changed = (evidence.ready_arm_mask & bit) != 0;
        let poll_changed = (evidence.poll_ready_arm_mask & bit) != 0;
        if ready_changed {
            evidence.ready_arm_mask &= !bit;
        }
        if poll_changed {
            evidence.poll_ready_arm_mask &= !bit;
        }
        ready_changed || poll_changed
    }

    #[inline]
    pub(super) fn peek_frame_hint(&self, slot: usize) -> Option<u8> {
        let evidence = *self.get(slot)?;
        if (evidence.flags & ScopeEvidence::FLAG_HINT_CONFLICT) != 0 {
            return None;
        }
        if (evidence.flags & ScopeEvidence::FLAG_HAS_HINT) != 0 {
            Some(evidence.hint_frame_label)
        } else {
            None
        }
    }

    #[inline]
    pub(super) fn peek_frame_hint_with_lane(&self, slot: usize) -> Option<(u8, u8)> {
        let evidence = *self.get(slot)?;
        if (evidence.flags & ScopeEvidence::FLAG_HINT_CONFLICT) != 0 {
            return None;
        }
        if (evidence.flags & ScopeEvidence::FLAG_HAS_HINT) != 0 {
            Some((evidence.hint_lane, evidence.hint_frame_label))
        } else {
            None
        }
    }

    #[inline]
    pub(super) fn conflicted(&self, slot: usize) -> bool {
        (self[slot].flags & (ScopeEvidence::FLAG_ACK_CONFLICT | ScopeEvidence::FLAG_HINT_CONFLICT))
            != 0
    }
}
