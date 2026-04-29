//! Authority-path helpers for resolver/ack/poll decisions.

use crate::{
    endpoint::{SendError, SendResult},
    global::const_dsl::ScopeId,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LoopDecision {
    Continue,
    Break,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Arm(u8);

impl Arm {
    #[inline]
    pub(super) const fn new(value: u8) -> Option<Self> {
        if value <= 1 { Some(Self(value)) } else { None }
    }

    #[inline]
    pub(super) const fn as_u8(self) -> u8 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RouteDecisionSource {
    Ack,
    Resolver,
    Poll,
}

impl RouteDecisionSource {
    #[inline]
    pub(super) const fn as_tap_seq(self) -> u8 {
        match self {
            Self::Ack => 1,
            Self::Resolver => 2,
            Self::Poll => 3,
        }
    }

    #[cfg(test)]
    #[inline]
    pub(super) const fn from_tap_seq(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::Ack),
            2 => Some(Self::Resolver),
            3 => Some(Self::Poll),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct RouteDecisionToken {
    arm: Arm,
    source: RouteDecisionSource,
}

impl RouteDecisionToken {
    #[inline]
    pub(super) const fn from_ack(arm: Arm) -> Self {
        Self {
            arm,
            source: RouteDecisionSource::Ack,
        }
    }

    #[inline]
    pub(super) const fn from_resolver(arm: Arm) -> Self {
        Self {
            arm,
            source: RouteDecisionSource::Resolver,
        }
    }

    #[inline]
    pub(super) const fn from_poll(arm: Arm) -> Self {
        Self {
            arm,
            source: RouteDecisionSource::Poll,
        }
    }

    #[inline]
    pub(super) const fn arm(self) -> Arm {
        self.arm
    }

    #[inline]
    pub(super) const fn source(self) -> RouteDecisionSource {
        self.source
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RouteResolveStep {
    Resolved(Arm),
    Deferred { retry_hint: u8, source: DeferSource },
    Abort(u16),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DeferSource {
    Resolver,
}

impl DeferSource {
    const RESOLVER_AUDIT_TAG: u8 = 0x80;

    #[inline]
    pub(super) const fn as_audit_tag(self) -> u8 {
        match self {
            Self::Resolver => Self::RESOLVER_AUDIT_TAG,
        }
    }

    #[cfg(test)]
    #[inline]
    pub(super) const fn from_audit_tag(value: u8) -> Option<Self> {
        match value {
            Self::RESOLVER_AUDIT_TAG => Some(Self::Resolver),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DeferReason {
    Unsupported = 1,
    NoEvidence = 2,
}

#[inline]
pub(super) fn route_policy_input_arg0(input: &[u32; 4]) -> u32 {
    input[0]
}

#[inline]
pub(super) fn validate_route_decision_scope(
    scope: ScopeId,
    policy_scope: ScopeId,
) -> SendResult<()> {
    if scope.is_none() {
        return Err(SendError::PhaseInvariant);
    }
    if !policy_scope.is_none() && scope != policy_scope {
        return Err(SendError::PhaseInvariant);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::RouteDecisionSource;

    #[test]
    fn route_authority_exactly_ack_resolver_poll() {
        assert_eq!(RouteDecisionSource::Ack.as_tap_seq(), 1);
        assert_eq!(RouteDecisionSource::Resolver.as_tap_seq(), 2);
        assert_eq!(RouteDecisionSource::Poll.as_tap_seq(), 3);

        assert_eq!(
            RouteDecisionSource::from_tap_seq(1),
            Some(RouteDecisionSource::Ack)
        );
        assert_eq!(
            RouteDecisionSource::from_tap_seq(2),
            Some(RouteDecisionSource::Resolver)
        );
        assert_eq!(
            RouteDecisionSource::from_tap_seq(3),
            Some(RouteDecisionSource::Poll)
        );

        for forbidden in [0, 4, u8::MAX] {
            assert_eq!(RouteDecisionSource::from_tap_seq(forbidden), None);
        }
    }
}
