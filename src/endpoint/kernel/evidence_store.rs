//! Mutable scope-evidence owner for endpoint kernel runtime bookkeeping.

use super::authority::RouteDecisionToken;
use super::evidence::ScopeEvidence;

#[cfg(feature = "std")]
fn boxed_repeat_array<T: Clone, const N: usize>(value: T) -> std::boxed::Box<[T; N]> {
    let values: std::boxed::Box<[T]> = std::vec![value; N].into_boxed_slice();
    match values.try_into() {
        Ok(fixed) => fixed,
        Err(_) => panic!("fixed array length"),
    }
}

pub(super) struct ScopeEvidenceStore {
    #[cfg(feature = "std")]
    pub(super) scope_evidence: std::boxed::Box<[ScopeEvidence; crate::eff::meta::MAX_EFF_NODES]>,
    #[cfg(not(feature = "std"))]
    pub(super) scope_evidence: [ScopeEvidence; crate::eff::meta::MAX_EFF_NODES],
    #[cfg(feature = "std")]
    pub(super) scope_evidence_generations: std::boxed::Box<[u32; crate::eff::meta::MAX_EFF_NODES]>,
    #[cfg(not(feature = "std"))]
    pub(super) scope_evidence_generations: [u32; crate::eff::meta::MAX_EFF_NODES],
}

impl ScopeEvidenceStore {
    #[cfg(feature = "std")]
    pub(super) fn new() -> Self {
        Self {
            scope_evidence: boxed_repeat_array(ScopeEvidence::EMPTY),
            scope_evidence_generations: boxed_repeat_array(0u32),
        }
    }

    #[cfg(not(feature = "std"))]
    pub(super) fn new() -> Self {
        Self {
            scope_evidence: [ScopeEvidence::EMPTY; crate::eff::meta::MAX_EFF_NODES],
            scope_evidence_generations: [0; crate::eff::meta::MAX_EFF_NODES],
        }
    }

    #[inline]
    pub(super) fn generation(&self, slot: Option<usize>) -> u32 {
        slot.and_then(|slot| self.scope_evidence_generations.get(slot).copied())
            .unwrap_or(0)
    }

    #[inline]
    pub(super) fn bump_generation(&mut self, slot: usize) {
        let Some(generation) = self.scope_evidence_generations.get_mut(slot) else {
            return;
        };
        let next = generation.wrapping_add(1);
        *generation = if next == 0 { 1 } else { next };
    }

    #[inline]
    pub(super) fn record_ack(&mut self, slot: usize, token: RouteDecisionToken) -> bool {
        let Some(evidence) = self.scope_evidence.get_mut(slot) else {
            return false;
        };
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
        let evidence = *self.scope_evidence.get(slot)?;
        if (evidence.flags & ScopeEvidence::FLAG_ACK_CONFLICT) != 0 {
            return None;
        }
        evidence.ack
    }

    #[inline]
    pub(super) fn take_ack(&mut self, slot: usize) -> Option<RouteDecisionToken> {
        let evidence = self.scope_evidence.get_mut(slot)?;
        if (evidence.flags & ScopeEvidence::FLAG_ACK_CONFLICT) != 0 {
            return None;
        }
        let token = evidence.ack;
        evidence.ack = None;
        token
    }

    #[inline]
    pub(super) fn record_hint(&mut self, slot: usize, label: u8) -> bool {
        let Some(evidence) = self.scope_evidence.get_mut(slot) else {
            return false;
        };
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
        let Some(evidence) = self.scope_evidence.get_mut(slot) else {
            return false;
        };
        let old_label = evidence.hint_label;
        let old_flags = evidence.flags;
        evidence.hint_label = label;
        evidence.flags &= !ScopeEvidence::FLAG_HINT_CONFLICT;
        evidence.hint_label != old_label || evidence.flags != old_flags
    }

    #[inline]
    pub(super) fn mark_ready_arm(&mut self, slot: usize, arm: u8, poll_ready: bool) -> bool {
        let Some(evidence) = self.scope_evidence.get_mut(slot) else {
            return false;
        };
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
        self.scope_evidence
            .get(slot)
            .map(|evidence| evidence.ready_arm_mask)
            .unwrap_or(0)
    }

    #[inline]
    pub(super) fn poll_ready_arm_mask(&self, slot: usize) -> u8 {
        self.scope_evidence
            .get(slot)
            .map(|evidence| evidence.poll_ready_arm_mask)
            .unwrap_or(0)
    }

    #[inline]
    pub(super) fn consume_ready_arm(&mut self, slot: usize, arm: u8) -> bool {
        let Some(evidence) = self.scope_evidence.get_mut(slot) else {
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
        let evidence = *self.scope_evidence.get(slot)?;
        if (evidence.flags & ScopeEvidence::FLAG_HINT_CONFLICT) != 0 {
            return None;
        }
        if evidence.hint_label == ScopeEvidence::NONE {
            None
        } else {
            Some(evidence.hint_label)
        }
    }

    #[inline]
    pub(super) fn take_hint(&mut self, slot: usize) -> Option<u8> {
        let evidence = self.scope_evidence.get_mut(slot)?;
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
    pub(super) fn clear(&mut self, slot: usize) -> bool {
        let Some(evidence) = self.scope_evidence.get(slot).copied() else {
            return false;
        };
        let changed = evidence.ack.is_some()
            || evidence.hint_label != ScopeEvidence::NONE
            || evidence.ready_arm_mask != 0
            || evidence.poll_ready_arm_mask != 0
            || evidence.flags != 0;
        if changed {
            self.scope_evidence[slot] = ScopeEvidence::EMPTY;
        }
        changed
    }

    #[inline]
    pub(super) fn conflicted(&self, slot: usize) -> bool {
        self.scope_evidence
            .get(slot)
            .map(|evidence| evidence.flags != 0)
            .unwrap_or(false)
    }
}
