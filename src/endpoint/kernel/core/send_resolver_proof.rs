use super::{SendError, SendResult};
use crate::global::const_dsl::ScopeId;

#[derive(Clone, Copy)]
pub(crate) struct ResolverDecisionProof {
    scope: ScopeId,
    resolver_id: u16,
    arm: u8,
    lane: u8,
}

impl ResolverDecisionProof {
    const EMPTY: Self = Self {
        scope: ScopeId::none(),
        resolver_id: 0,
        arm: u8::MAX,
        lane: u8::MAX,
    };

    #[inline]
    pub(crate) const fn new(scope: ScopeId, resolver_id: u16, arm: u8, lane: u8) -> Self {
        if scope.is_none() || arm > 1 {
            crate::invariant();
        }
        Self {
            scope,
            resolver_id,
            arm,
            lane,
        }
    }

    #[inline]
    pub(crate) const fn scope(self) -> ScopeId {
        self.scope
    }

    #[inline]
    pub(crate) const fn resolver_id(self) -> u16 {
        self.resolver_id
    }

    #[inline]
    pub(crate) const fn arm(self) -> u8 {
        self.arm
    }

    #[inline]
    pub(crate) const fn lane(self) -> u8 {
        self.lane
    }

    #[inline]
    const fn is_empty(self) -> bool {
        self.scope.is_none()
    }

    #[inline]
    const fn same_site(self, scope: ScopeId, resolver_id: u16) -> bool {
        !self.is_empty() && self.scope.same(scope) && self.resolver_id == resolver_id
    }

    #[inline]
    const fn matches(self, scope: ScopeId, resolver_id: u16, arm: u8, lane: u8) -> bool {
        self.same_site(scope, resolver_id) && self.arm == arm && self.lane == lane
    }

    #[inline]
    pub(crate) const fn decision_arm(self) -> crate::session::cluster::core::DecisionArm {
        match self.arm {
            0 => crate::session::cluster::core::DecisionArm::Left,
            1 => crate::session::cluster::core::DecisionArm::Right,
            _ => crate::invariant(),
        }
    }
}

const MAX_SEND_RESOLVER_DECISION_PROOFS: usize = 4;

#[derive(Clone, Copy)]
pub(crate) struct ResolverDecisionProofs {
    len: u8,
    proofs: [ResolverDecisionProof; MAX_SEND_RESOLVER_DECISION_PROOFS],
}

impl ResolverDecisionProofs {
    #[inline]
    pub(crate) const fn empty() -> Self {
        Self {
            len: 0,
            proofs: [ResolverDecisionProof::EMPTY; MAX_SEND_RESOLVER_DECISION_PROOFS],
        }
    }

    #[inline]
    pub(crate) const fn len(self) -> usize {
        self.len as usize
    }

    #[inline]
    pub(crate) const fn is_empty(self) -> bool {
        self.len == 0
    }

    #[inline]
    pub(crate) fn get(self, idx: usize) -> Option<ResolverDecisionProof> {
        if idx >= self.len() {
            None
        } else {
            Some(self.proofs[idx])
        }
    }

    #[inline]
    pub(crate) fn push(&mut self, proof: ResolverDecisionProof) -> SendResult<()> {
        if proof.is_empty() {
            return Err(SendError::PhaseInvariant);
        }
        let mut idx = 0usize;
        while idx < self.len() {
            let existing = self.proofs[idx];
            if existing.same_site(proof.scope(), proof.resolver_id()) {
                if existing.matches(
                    proof.scope(),
                    proof.resolver_id(),
                    proof.arm(),
                    proof.lane(),
                ) {
                    return Ok(());
                }
                return Err(SendError::PhaseInvariant);
            }
            idx += 1;
        }
        if self.len() >= MAX_SEND_RESOLVER_DECISION_PROOFS {
            return Err(SendError::PhaseInvariant);
        }
        self.proofs[self.len()] = proof;
        self.len += 1;
        Ok(())
    }

    #[inline]
    pub(crate) fn match_index(
        self,
        scope: ScopeId,
        resolver_id: u16,
        arm: u8,
        lane: u8,
    ) -> Option<usize> {
        let mut idx = 0usize;
        while idx < self.len() {
            if self.proofs[idx].matches(scope, resolver_id, arm, lane) {
                return Some(idx);
            }
            idx += 1;
        }
        None
    }

    #[inline]
    pub(crate) fn site_index(self, scope: ScopeId, resolver_id: u16) -> Option<usize> {
        let mut idx = 0usize;
        while idx < self.len() {
            if self.proofs[idx].same_site(scope, resolver_id) {
                return Some(idx);
            }
            idx += 1;
        }
        None
    }
}
