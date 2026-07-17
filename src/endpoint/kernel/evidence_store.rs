//! Mutable scope-evidence owner for endpoint kernel runtime bookkeeping.

use super::authority::Arm;
use super::evidence::{ScopeEvidence, ScopeEvidenceStatus};
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
        self[slot].clear()
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
    pub(super) fn mark_ready_arm(
        &mut self,
        slot: usize,
        arm: Arm,
        selected: Option<Arm>,
        evidence_kind: ReadyArmEvidence,
    ) -> bool {
        self[slot].mark_ready_arm(arm, selected, evidence_kind)
    }

    #[inline]
    pub(super) fn ready_arm_mask(&self, slot: usize) -> u8 {
        self[slot].ready_arm_mask()
    }

    #[inline]
    pub(super) fn poll_ready_arm_mask(&self, slot: usize) -> u8 {
        self[slot].poll_ready_arm_mask()
    }

    #[inline]
    pub(super) fn consume_ready_arm(&mut self, slot: usize, arm: Arm) -> bool {
        self[slot].consume_ready_arm(arm)
    }

    #[inline]
    pub(super) fn conflicted(&self, slot: usize) -> bool {
        self[slot].conflicted()
    }

    #[inline]
    pub(super) fn selection_is_coherent(&self, slot: usize, arm: Arm) -> bool {
        self[slot].selection_is_coherent(arm)
    }
}

impl ScopeEvidence {
    #[inline]
    fn clear(&mut self) -> bool {
        let changed = self.ready_arm_mask != 0
            || self.poll_ready_arm_mask != 0
            || self.status != ScopeEvidenceStatus::Clear;
        if changed {
            *self = Self::EMPTY;
        }
        changed
    }

    #[inline]
    fn mask_is_ambiguous(mask: u8) -> bool {
        mask & !0b11 != 0 || mask == 0b11
    }

    #[inline]
    fn conflicted(self) -> bool {
        self.status == ScopeEvidenceStatus::Conflicted
            || Self::mask_is_ambiguous(self.ready_arm_mask)
    }

    #[inline]
    fn selection_is_coherent(self, arm: Arm) -> bool {
        !self.conflicted()
            && Arm::from_single_ready_mask(self.ready_arm_mask()).is_none_or(|ready| ready == arm)
    }

    #[inline]
    fn mark_conflicted(&mut self) {
        self.status = ScopeEvidenceStatus::Conflicted;
        self.ready_arm_mask = 0;
        self.poll_ready_arm_mask = 0;
    }

    #[inline]
    fn conflicts_with(self, arm: Arm) -> bool {
        let bit = Self::arm_bit(arm);
        self.ready_arm_mask & !bit != 0
    }

    #[inline]
    fn mark_ready_arm(
        &mut self,
        arm: Arm,
        selected: Option<Arm>,
        evidence_kind: ReadyArmEvidence,
    ) -> bool {
        if self.status == ScopeEvidenceStatus::Conflicted {
            return false;
        }
        if selected.is_some_and(|selected| selected != arm)
            || Self::mask_is_ambiguous(self.ready_arm_mask)
            || self.conflicts_with(arm)
        {
            self.mark_conflicted();
            return true;
        }
        let bit = Self::arm_bit(arm);
        let ready_changed = (self.ready_arm_mask & bit) == 0;
        let poll_changed = evidence_kind.records_poll() && (self.poll_ready_arm_mask & bit) == 0;
        if ready_changed {
            self.ready_arm_mask |= bit;
        }
        if poll_changed {
            self.poll_ready_arm_mask |= bit;
        }
        ready_changed || poll_changed
    }

    #[inline]
    fn ready_arm_mask(self) -> u8 {
        if self.conflicted() {
            0
        } else {
            self.ready_arm_mask
        }
    }

    #[inline]
    fn poll_ready_arm_mask(self) -> u8 {
        if self.conflicted() {
            0
        } else {
            self.poll_ready_arm_mask
        }
    }

    #[inline]
    fn consume_ready_arm(&mut self, arm: Arm) -> bool {
        if self.conflicted() {
            return false;
        }
        let bit = Self::arm_bit(arm);
        let ready_changed = (self.ready_arm_mask & bit) != 0;
        let poll_changed = (self.poll_ready_arm_mask & bit) != 0;
        if ready_changed {
            self.ready_arm_mask &= !bit;
        }
        if poll_changed {
            self.poll_ready_arm_mask &= !bit;
        }
        ready_changed || poll_changed
    }

    #[inline]
    #[cfg(any(kani, all(test, hibana_repo_tests)))]
    fn masks_are_canonical(self) -> bool {
        if self.conflicted() {
            self.ready_arm_mask() == 0 && self.poll_ready_arm_mask() == 0
        } else {
            !Self::mask_is_ambiguous(self.ready_arm_mask)
                && self.poll_ready_arm_mask & !self.ready_arm_mask == 0
        }
    }
}

#[cfg(any(kani, all(test, hibana_repo_tests)))]
mod tests {
    use super::*;

    #[cfg(all(test, hibana_repo_tests))]
    #[test]
    fn distinct_ready_arms_poison_all_scope_authority() {
        let mut evidence = ScopeEvidence::EMPTY;
        assert!(evidence.mark_ready_arm(Arm::LEFT, None, ReadyArmEvidence::Poll));
        assert!(evidence.mark_ready_arm(Arm::RIGHT, None, ReadyArmEvidence::Materialization));
        assert!(evidence.conflicted());
        assert_eq!(evidence.ready_arm_mask(), 0);
        assert_eq!(evidence.poll_ready_arm_mask(), 0);
        assert!(!evidence.mark_ready_arm(Arm::LEFT, None, ReadyArmEvidence::Poll));
    }

    #[cfg(all(test, hibana_repo_tests))]
    #[test]
    fn matching_ready_arm_evidence_preserves_one_exact_authority() {
        let mut evidence = ScopeEvidence::EMPTY;
        assert!(evidence.mark_ready_arm(
            Arm::LEFT,
            Some(Arm::LEFT),
            ReadyArmEvidence::Materialization
        ));
        assert!(evidence.mark_ready_arm(Arm::LEFT, Some(Arm::LEFT), ReadyArmEvidence::Poll));
        assert!(!evidence.conflicted());
        assert_eq!(evidence.ready_arm_mask(), 1);
        assert_eq!(evidence.poll_ready_arm_mask(), 1);
    }

    #[cfg(all(test, hibana_repo_tests))]
    #[test]
    fn scope_evidence_is_three_bytes_per_projected_route_scope() {
        assert_eq!(core::mem::size_of::<ScopeEvidence>(), 3);
    }

    #[cfg(all(test, hibana_repo_tests))]
    #[test]
    fn ready_arm_conflicting_with_live_selection_is_rejected() {
        let mut evidence = ScopeEvidence::EMPTY;
        assert!(evidence.mark_ready_arm(Arm::RIGHT, Some(Arm::LEFT), ReadyArmEvidence::Poll));
        assert!(evidence.conflicted());
        assert_eq!(evidence.ready_arm_mask(), 0);
        assert!(!evidence.selection_is_coherent(Arm::LEFT));
    }

    #[cfg(all(test, hibana_repo_tests))]
    #[test]
    fn poll_materialization_consume_and_clear_preserve_canonical_masks() {
        let mut evidence = ScopeEvidence::EMPTY;
        assert!(evidence.mark_ready_arm(Arm::LEFT, None, ReadyArmEvidence::Materialization));
        assert_eq!(evidence.ready_arm_mask(), 1);
        assert_eq!(evidence.poll_ready_arm_mask(), 0);
        assert!(evidence.masks_are_canonical());
        assert!(evidence.consume_ready_arm(Arm::LEFT));
        assert!(evidence.masks_are_canonical());
        assert!(evidence.mark_ready_arm(Arm::RIGHT, None, ReadyArmEvidence::Poll));
        assert_eq!(evidence.ready_arm_mask(), 2);
        assert_eq!(evidence.poll_ready_arm_mask(), 2);
        assert!(evidence.masks_are_canonical());
        assert!(evidence.clear());
        assert_eq!(evidence, ScopeEvidence::EMPTY);
    }

    #[cfg(kani)]
    #[kani::proof]
    fn distinct_ready_arms_are_sticky_conflict_in_either_order() {
        let first = if kani::any::<bool>() {
            Arm::LEFT
        } else {
            Arm::RIGHT
        };
        let second = if first == Arm::LEFT {
            Arm::RIGHT
        } else {
            Arm::LEFT
        };
        let mut evidence = ScopeEvidence::EMPTY;
        evidence.mark_ready_arm(first, None, ReadyArmEvidence::Poll);
        evidence.mark_ready_arm(second, None, ReadyArmEvidence::Materialization);
        assert!(evidence.conflicted());
        assert_eq!(evidence.ready_arm_mask(), 0);
        assert_eq!(evidence.poll_ready_arm_mask(), 0);
        assert!(!evidence.mark_ready_arm(first, None, ReadyArmEvidence::Poll));
        assert!(evidence.conflicted());
    }

    #[cfg(kani)]
    #[kani::proof]
    fn matching_ready_arm_evidence_remains_exact() {
        let arm = if kani::any::<bool>() {
            Arm::LEFT
        } else {
            Arm::RIGHT
        };
        let mut evidence = ScopeEvidence::EMPTY;
        evidence.mark_ready_arm(arm, Some(arm), ReadyArmEvidence::Materialization);
        evidence.mark_ready_arm(arm, Some(arm), ReadyArmEvidence::Poll);
        assert!(!evidence.conflicted());
        assert_eq!(evidence.ready_arm_mask(), ScopeEvidence::arm_bit(arm));
        assert_eq!(evidence.poll_ready_arm_mask(), ScopeEvidence::arm_bit(arm));
    }

    #[cfg(kani)]
    #[kani::proof]
    fn ready_arm_conflicting_with_live_selection_is_sticky() {
        let selected = if kani::any::<bool>() {
            Arm::LEFT
        } else {
            Arm::RIGHT
        };
        let incoming = if selected == Arm::LEFT {
            Arm::RIGHT
        } else {
            Arm::LEFT
        };
        let mut evidence = ScopeEvidence::EMPTY;
        evidence.mark_ready_arm(incoming, Some(selected), ReadyArmEvidence::Poll);
        assert!(evidence.conflicted());
        assert!(!evidence.selection_is_coherent(selected));
        assert!(!evidence.mark_ready_arm(selected, Some(selected), ReadyArmEvidence::Poll));
        assert!(evidence.conflicted());
    }

    #[cfg(kani)]
    #[kani::proof]
    fn ready_evidence_transitions_preserve_canonical_masks() {
        let first = if kani::any::<bool>() {
            Arm::LEFT
        } else {
            Arm::RIGHT
        };
        let second = if kani::any::<bool>() {
            Arm::LEFT
        } else {
            Arm::RIGHT
        };
        let selected = match kani::any::<u8>() % 3 {
            0 => None,
            1 => Some(Arm::LEFT),
            _ => Some(Arm::RIGHT),
        };
        let first_kind = if kani::any::<bool>() {
            ReadyArmEvidence::Poll
        } else {
            ReadyArmEvidence::Materialization
        };
        let second_kind = if kani::any::<bool>() {
            ReadyArmEvidence::Poll
        } else {
            ReadyArmEvidence::Materialization
        };
        let mut evidence = ScopeEvidence::EMPTY;

        evidence.mark_ready_arm(first, selected, first_kind);
        assert!(evidence.masks_are_canonical());
        evidence.mark_ready_arm(second, selected, second_kind);
        assert!(evidence.masks_are_canonical());
        evidence.consume_ready_arm(first);
        assert!(evidence.masks_are_canonical());
        evidence.clear();
        assert_eq!(evidence, ScopeEvidence::EMPTY);
        assert!(evidence.masks_are_canonical());
    }

    #[cfg(kani)]
    #[kani::proof]
    fn poll_and_materialization_consumption_are_exact() {
        let arm = if kani::any::<bool>() {
            Arm::LEFT
        } else {
            Arm::RIGHT
        };
        let bit = ScopeEvidence::arm_bit(arm);
        let mut evidence = ScopeEvidence::EMPTY;

        assert!(evidence.mark_ready_arm(arm, None, ReadyArmEvidence::Materialization));
        assert_eq!(evidence.ready_arm_mask(), bit);
        assert_eq!(evidence.poll_ready_arm_mask(), 0);
        assert!(evidence.consume_ready_arm(arm));
        assert_eq!(evidence, ScopeEvidence::EMPTY);

        assert!(evidence.mark_ready_arm(arm, None, ReadyArmEvidence::Poll));
        assert_eq!(evidence.ready_arm_mask(), bit);
        assert_eq!(evidence.poll_ready_arm_mask(), bit);
        assert!(evidence.consume_ready_arm(arm));
        assert_eq!(evidence, ScopeEvidence::EMPTY);
        assert!(!evidence.consume_ready_arm(arm));
    }
}
