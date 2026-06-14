//! Authority-path helpers for resolver/ack/poll decisions.

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
pub(super) enum RouteArmToken {
    Ack(Arm),
    Resolver(Arm),
    Poll(Arm),
}

impl RouteArmToken {
    #[inline]
    pub(super) const fn from_ack(arm: Arm) -> Self {
        Self::Ack(arm)
    }

    #[inline]
    pub(super) const fn from_resolver(arm: Arm) -> Self {
        Self::Resolver(arm)
    }

    #[inline]
    pub(super) const fn from_poll(arm: Arm) -> Self {
        Self::Poll(arm)
    }

    #[inline]
    pub(super) const fn arm(self) -> Arm {
        match self {
            Self::Ack(arm) | Self::Resolver(arm) | Self::Poll(arm) => arm,
        }
    }

    #[inline]
    pub(super) const fn is_ack(self) -> bool {
        matches!(self, Self::Ack(_))
    }

    #[inline]
    pub(super) const fn is_resolver(self) -> bool {
        matches!(self, Self::Resolver(_))
    }

    #[inline]
    pub(super) const fn is_poll(self) -> bool {
        matches!(self, Self::Poll(_))
    }

    #[inline]
    pub(super) const fn as_tap_seq(self) -> u8 {
        match self {
            Self::Ack(_) => 1,
            Self::Resolver(_) => 2,
            Self::Poll(_) => 3,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RouteResolveStep {
    Resolved(Arm),
    Deferred,
    Reject(u16),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DeferReason {
    ResolverDeferred = 1,
    EvidenceAbsent = 2,
}

#[cfg(test)]
mod tests {
    use super::{Arm, RouteArmToken};

    #[test]
    fn route_arm_token_carries_arm_and_authority_together() {
        let left = Arm::new(0).expect("left arm");
        assert_eq!(RouteArmToken::from_ack(left).arm(), left);
        assert_eq!(RouteArmToken::from_ack(left).as_tap_seq(), 1);
        assert_eq!(RouteArmToken::from_resolver(left).as_tap_seq(), 2);
        assert_eq!(RouteArmToken::from_poll(left).as_tap_seq(), 3);
        assert!(RouteArmToken::from_ack(left).is_ack());
        assert!(RouteArmToken::from_resolver(left).is_resolver());
        assert!(RouteArmToken::from_poll(left).is_poll());
    }
}
