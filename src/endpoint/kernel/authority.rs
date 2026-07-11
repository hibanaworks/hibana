//! Authority-path helpers for resolver/ack/poll decisions.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Arm(u8);

impl Arm {
    pub(super) const LEFT: Self = Self(0);
    pub(super) const RIGHT: Self = Self(1);

    #[inline]
    pub(super) const fn decode_raw(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::LEFT),
            1 => Some(Self::RIGHT),
            2..=u8::MAX => None,
        }
    }

    #[inline]
    pub(super) const fn from_raw(value: u8) -> Self {
        match Self::decode_raw(value) {
            Some(arm) => arm,
            None => crate::invariant(),
        }
    }

    #[inline]
    const fn decode_single_ready_mask(mask: u8) -> Option<Option<Self>> {
        match mask {
            0 | 3 => Some(None),
            1 => Some(Some(Self::LEFT)),
            2 => Some(Some(Self::RIGHT)),
            4..=u8::MAX => None,
        }
    }

    #[inline]
    pub(super) const fn from_single_ready_mask(mask: u8) -> Option<Self> {
        match Self::decode_single_ready_mask(mask) {
            Some(arm) => arm,
            None => crate::invariant(),
        }
    }

    #[inline]
    pub(super) const fn as_u8(self) -> u8 {
        self.0
    }
}

#[cfg(kani)]
mod kani;

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
    Reject(u16),
}

#[cfg(test)]
mod tests {
    use super::{Arm, RouteArmToken};

    #[test]
    fn route_arm_token_carries_arm_and_authority_together() {
        let left = Arm::LEFT;
        assert_eq!(RouteArmToken::from_ack(left).arm(), left);
        assert_eq!(RouteArmToken::from_ack(left).as_tap_seq(), 1);
        assert_eq!(RouteArmToken::from_resolver(left).as_tap_seq(), 2);
        assert_eq!(RouteArmToken::from_poll(left).as_tap_seq(), 3);
        assert!(RouteArmToken::from_ack(left).is_ack());
        assert!(RouteArmToken::from_resolver(left).is_resolver());
        assert!(RouteArmToken::from_poll(left).is_poll());
    }

    #[test]
    fn raw_route_arm_and_ready_mask_decode_exact_binary_authority() {
        for raw in 0..=u8::MAX {
            assert_eq!(Arm::decode_raw(raw).is_some(), raw <= 1);
            let expected = match raw {
                0 | 3 => Some(None),
                1 => Some(Some(Arm::LEFT)),
                2 => Some(Some(Arm::RIGHT)),
                4..=u8::MAX => None,
            };
            assert_eq!(Arm::decode_single_ready_mask(raw), expected);
        }
        assert_eq!(Arm::from_single_ready_mask(0), None);
        assert_eq!(Arm::from_single_ready_mask(1), Some(Arm::LEFT));
        assert_eq!(Arm::from_single_ready_mask(2), Some(Arm::RIGHT));
        assert_eq!(Arm::from_single_ready_mask(3), None);
    }

    #[test]
    #[should_panic]
    fn invalid_raw_route_arm_fails_closed() {
        let _ = Arm::from_raw(2);
    }

    #[test]
    #[should_panic]
    fn invalid_single_bit_ready_mask_fails_closed() {
        let _ = Arm::from_single_ready_mask(1 << 2);
    }

    #[test]
    #[should_panic]
    fn invalid_mixed_ready_mask_fails_closed() {
        let _ = Arm::from_single_ready_mask((1 << 2) | 1);
    }
}
