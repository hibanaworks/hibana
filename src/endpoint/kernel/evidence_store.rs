//! Mutable scope-evidence owner for endpoint kernel runtime bookkeeping.

use super::authority::RouteDecisionToken;
use super::evidence::ScopeEvidence;
use core::ops::{Index, IndexMut};

#[derive(Clone, Copy)]
pub(super) struct ScopeEvidenceSlot {
    evidence: ScopeEvidence,
    generation: u16,
}

impl ScopeEvidenceSlot {
    const EMPTY: Self = Self {
        evidence: ScopeEvidence::EMPTY,
        generation: 0,
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
            panic!("scope evidence capacity overflow");
        }
        unsafe {
            core::ptr::addr_of_mut!((*dst).slots).write(slots);
            core::ptr::addr_of_mut!((*dst).len).write(len as u16);
        }
        let mut idx = 0usize;
        while idx < len {
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
    pub(super) fn get(&self, slot: usize) -> Option<&ScopeEvidence> {
        if !self.contains(slot) {
            return None;
        }
        Some(unsafe { &(*self.slots.add(slot)).evidence })
    }

    #[inline]
    pub(super) fn get_mut(&mut self, slot: usize) -> Option<&mut ScopeEvidence> {
        if !self.contains(slot) {
            return None;
        }
        Some(unsafe { &mut (*self.slots.add(slot)).evidence })
    }

    #[inline]
    pub(super) fn generation(&self, slot: usize) -> u16 {
        if !self.contains(slot) {
            return 0;
        }
        unsafe { (*self.slots.add(slot)).generation }
    }

    pub(super) fn bump_generation(&mut self, slot: usize) {
        if !self.contains(slot) {
            return;
        }
        let generation = unsafe { &mut (*self.slots.add(slot)).generation };
        let next = generation.wrapping_add(1);
        *generation = if next == 0 { 1 } else { next };
    }

    pub(super) fn clear(&mut self, slot: usize) -> bool {
        let Some(evidence) = self.get_mut(slot) else {
            return false;
        };
        let changed = evidence.ack.is_some()
            || evidence.hint_label != ScopeEvidence::NONE
            || evidence.ready_arm_mask != 0
            || evidence.poll_ready_arm_mask != 0
            || evidence.flags != 0;
        if changed {
            *evidence = ScopeEvidence::EMPTY;
        }
        changed
    }
}

static EMPTY_SCOPE_EVIDENCE: ScopeEvidence = ScopeEvidence::EMPTY;

impl Index<usize> for ScopeEvidenceTable {
    type Output = ScopeEvidence;

    #[inline]
    fn index(&self, index: usize) -> &Self::Output {
        self.get(index).unwrap_or(&EMPTY_SCOPE_EVIDENCE)
    }
}

impl IndexMut<usize> for ScopeEvidenceTable {
    #[inline]
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        self.get_mut(index)
            .expect("scope evidence slot must fit compiled dense route scope bound")
    }
}

impl ScopeEvidenceTable {
    #[inline]
    pub(super) fn generation_for_slot(&self, slot: Option<usize>) -> u16 {
        slot.map(|slot| self.generation(slot)).unwrap_or(0)
    }

    #[inline]
    pub(super) fn record_ack(&mut self, slot: usize, token: RouteDecisionToken) -> bool {
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
    pub(super) fn peek_ack(&self, slot: usize) -> Option<RouteDecisionToken> {
        let evidence = *self.get(slot)?;
        if (evidence.flags & ScopeEvidence::FLAG_ACK_CONFLICT) != 0 {
            return None;
        }
        evidence.ack
    }

    #[inline]
    pub(super) fn take_ack(&mut self, slot: usize) -> Option<RouteDecisionToken> {
        let evidence = self.get_mut(slot)?;
        if (evidence.flags & ScopeEvidence::FLAG_ACK_CONFLICT) != 0 {
            return None;
        }
        let token = evidence.ack;
        evidence.ack = None;
        token
    }

    #[inline]
    pub(super) fn record_hint(&mut self, slot: usize, label: u8) -> bool {
        let evidence = &mut self[slot];
        if (evidence.flags & ScopeEvidence::FLAG_HINT_CONFLICT) != 0 {
            return false;
        }
        if evidence.hint_label == ScopeEvidence::NONE {
            evidence.hint_label = label;
            true
        } else if evidence.hint_label == label {
            false
        } else {
            evidence.flags |= ScopeEvidence::FLAG_HINT_CONFLICT;
            evidence.hint_label = ScopeEvidence::NONE;
            true
        }
    }

    #[inline]
    pub(super) fn record_hint_dynamic(&mut self, slot: usize, label: u8) -> bool {
        let evidence = &mut self[slot];
        let old_label = evidence.hint_label;
        let old_flags = evidence.flags;
        evidence.hint_label = label;
        evidence.flags &= !ScopeEvidence::FLAG_HINT_CONFLICT;
        evidence.hint_label != old_label || evidence.flags != old_flags
    }

    #[inline]
    pub(super) fn mark_ready_arm(&mut self, slot: usize, arm: u8, poll_ready: bool) -> bool {
        let evidence = &mut self[slot];
        let bit = ScopeEvidence::arm_bit(arm);
        let ready_changed = (evidence.ready_arm_mask & bit) == 0;
        let poll_changed = poll_ready && (evidence.poll_ready_arm_mask & bit) == 0;
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
        self.get(slot)
            .map(|evidence| evidence.ready_arm_mask)
            .unwrap_or(0)
    }

    #[inline]
    pub(super) fn poll_ready_arm_mask(&self, slot: usize) -> u8 {
        self.get(slot)
            .map(|evidence| evidence.poll_ready_arm_mask)
            .unwrap_or(0)
    }

    #[inline]
    pub(super) fn consume_ready_arm(&mut self, slot: usize, arm: u8) -> bool {
        let Some(evidence) = self.get_mut(slot) else {
            return false;
        };
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
    pub(super) fn peek_hint(&self, slot: usize) -> Option<u8> {
        let evidence = *self.get(slot)?;
        if (evidence.flags & ScopeEvidence::FLAG_HINT_CONFLICT) != 0 {
            return None;
        }
        if evidence.hint_label == ScopeEvidence::NONE {
            None
        } else {
            Some(evidence.hint_label)
        }
    }

    #[cfg(test)]
    #[inline]
    pub(super) fn take_hint(&mut self, slot: usize) -> Option<u8> {
        let evidence = self.get_mut(slot)?;
        if (evidence.flags & ScopeEvidence::FLAG_HINT_CONFLICT) != 0 {
            return None;
        }
        let label = evidence.hint_label;
        evidence.hint_label = ScopeEvidence::NONE;
        if label == ScopeEvidence::NONE {
            None
        } else {
            Some(label)
        }
    }

    #[inline]
    pub(super) fn conflicted(&self, slot: usize) -> bool {
        self.get(slot)
            .map(|evidence| evidence.flags != 0)
            .unwrap_or(false)
    }
}
