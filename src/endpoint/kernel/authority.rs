//! Authority-path helpers for resolver/ack/poll decisions.

use crate::transport::context::PolicyInput;

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
pub(super) enum RouteAuthoritySource {
    Ack,
    Resolver,
    Poll,
}

impl RouteAuthoritySource {
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
pub(super) struct RouteArmToken {
    arm: Arm,
    source: RouteAuthoritySource,
}

impl RouteArmToken {
    #[inline]
    pub(super) const fn from_ack(arm: Arm) -> Self {
        Self {
            arm,
            source: RouteAuthoritySource::Ack,
        }
    }

    #[inline]
    pub(super) const fn from_resolver(arm: Arm) -> Self {
        Self {
            arm,
            source: RouteAuthoritySource::Resolver,
        }
    }

    #[inline]
    pub(super) const fn from_poll(arm: Arm) -> Self {
        Self {
            arm,
            source: RouteAuthoritySource::Poll,
        }
    }

    #[inline]
    pub(super) const fn arm(self) -> Arm {
        self.arm
    }

    #[inline]
    pub(super) const fn source(self) -> RouteAuthoritySource {
        self.source
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RouteResolveStep {
    Resolved(Arm),
    Deferred { source: DeferSource },
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DeferReason {
    Unsupported = 1,
    NoEvidence = 2,
}

#[inline]
pub(super) fn decision_policy_input_arg0(input: PolicyInput) -> u32 {
    input.primary()
}

#[cfg(test)]
mod tests {
    use super::RouteAuthoritySource;

    #[test]
    fn route_authority_exactly_ack_resolver_poll() {
        assert_eq!(RouteAuthoritySource::Ack.as_tap_seq(), 1);
        assert_eq!(RouteAuthoritySource::Resolver.as_tap_seq(), 2);
        assert_eq!(RouteAuthoritySource::Poll.as_tap_seq(), 3);

        assert_eq!(
            RouteAuthoritySource::from_tap_seq(1),
            Some(RouteAuthoritySource::Ack)
        );
        assert_eq!(
            RouteAuthoritySource::from_tap_seq(2),
            Some(RouteAuthoritySource::Resolver)
        );
        assert_eq!(
            RouteAuthoritySource::from_tap_seq(3),
            Some(RouteAuthoritySource::Poll)
        );

        for forbidden in [0, 4, u8::MAX] {
            assert_eq!(RouteAuthoritySource::from_tap_seq(forbidden), None);
        }
    }
}
