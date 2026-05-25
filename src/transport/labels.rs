use core::ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign, Not};

/// Choreography-facing message / branch identity.
///
/// This is intentionally crate-private. Application code expresses logical
/// labels through `g::Msg<L, P, K>` and observes them through `RouteBranch`.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct LogicalLabel(u8);

impl LogicalLabel {
    #[inline]
    pub(crate) const fn new(raw: u8) -> Self {
        Self(raw)
    }

    #[inline]
    pub(crate) const fn raw(self) -> u8 {
        self.0
    }
}

/// Transport-facing discriminator for a projected local frame.
///
/// Application choreography labels remain logical branch/message identities.
/// `FrameLabel` is the compact demux value consumed by transports and bindings.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FrameLabel(u8);

impl FrameLabel {
    #[inline]
    pub const fn new(raw: u8) -> Self {
        Self(raw)
    }

    #[inline]
    pub const fn raw(self) -> u8 {
        self.0
    }
}

/// Fixed mask over the complete `FrameLabel` domain.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct FrameLabelMask {
    low: u128,
    high: u128,
}

impl FrameLabelMask {
    pub(crate) const EMPTY: Self = Self { low: 0, high: 0 };

    #[inline]
    pub(crate) const fn from_frame_label(frame_label: u8) -> Self {
        if frame_label < u128::BITS as u8 {
            Self {
                low: 1u128 << frame_label,
                high: 0,
            }
        } else {
            Self {
                low: 0,
                high: 1u128 << ((frame_label - u128::BITS as u8) as u32),
            }
        }
    }

    #[inline]
    pub(crate) const fn is_empty(self) -> bool {
        self.low == 0 && self.high == 0
    }

    #[inline]
    pub(crate) const fn contains_frame_label(self, frame_label: u8) -> bool {
        if frame_label < u128::BITS as u8 {
            (self.low & (1u128 << frame_label)) != 0
        } else {
            (self.high & (1u128 << ((frame_label - u128::BITS as u8) as u32))) != 0
        }
    }

    #[inline]
    pub(crate) const fn intersects(self, other: Self) -> bool {
        (self.low & other.low) != 0 || (self.high & other.high) != 0
    }

    #[inline]
    pub(crate) const fn without(self, other: Self) -> Self {
        Self {
            low: self.low & !other.low,
            high: self.high & !other.high,
        }
    }

    #[inline]
    pub(crate) fn insert_frame_label(&mut self, frame_label: u8) -> bool {
        let before = *self;
        *self |= Self::from_frame_label(frame_label);
        before != *self
    }

    #[inline]
    pub(crate) fn remove_frame_label(&mut self, frame_label: u8) {
        *self = self.without(Self::from_frame_label(frame_label));
    }

    #[inline]
    pub(crate) const fn singleton_frame_label(self) -> Option<u8> {
        if self.low != 0 {
            if self.high != 0 || (self.low & (self.low - 1)) != 0 {
                return None;
            }
            return Some(self.low.trailing_zeros() as u8);
        }
        if self.high == 0 || (self.high & (self.high - 1)) != 0 {
            return None;
        }
        Some((self.high.trailing_zeros() as u8) + u128::BITS as u8)
    }

    #[inline]
    pub(crate) fn take_matching<F>(&mut self, mut matches: F) -> Option<u8>
    where
        F: FnMut(u8) -> bool,
    {
        let mut remaining = self.low;
        while remaining != 0 {
            let frame_label = remaining.trailing_zeros() as u8;
            if matches(frame_label) {
                self.remove_frame_label(frame_label);
                return Some(frame_label);
            }
            remaining &= remaining - 1;
        }

        let mut remaining = self.high;
        while remaining != 0 {
            let frame_label = (remaining.trailing_zeros() as u8) + u128::BITS as u8;
            if matches(frame_label) {
                self.remove_frame_label(frame_label);
                return Some(frame_label);
            }
            remaining &= remaining - 1;
        }
        None
    }

    #[cfg(test)]
    pub(crate) fn has_matching<F>(self, mut matches: F) -> bool
    where
        F: FnMut(u8) -> bool,
    {
        let mut remaining = self.low;
        while remaining != 0 {
            let frame_label = remaining.trailing_zeros() as u8;
            if matches(frame_label) {
                return true;
            }
            remaining &= remaining - 1;
        }

        let mut remaining = self.high;
        while remaining != 0 {
            let frame_label = (remaining.trailing_zeros() as u8) + u128::BITS as u8;
            if matches(frame_label) {
                return true;
            }
            remaining &= remaining - 1;
        }
        false
    }
}

impl BitOr for FrameLabelMask {
    type Output = Self;

    #[inline]
    fn bitor(self, rhs: Self) -> Self::Output {
        Self {
            low: self.low | rhs.low,
            high: self.high | rhs.high,
        }
    }
}

impl BitOrAssign for FrameLabelMask {
    #[inline]
    fn bitor_assign(&mut self, rhs: Self) {
        self.low |= rhs.low;
        self.high |= rhs.high;
    }
}

impl BitAnd for FrameLabelMask {
    type Output = Self;

    #[inline]
    fn bitand(self, rhs: Self) -> Self::Output {
        Self {
            low: self.low & rhs.low,
            high: self.high & rhs.high,
        }
    }
}

impl BitAndAssign for FrameLabelMask {
    #[inline]
    fn bitand_assign(&mut self, rhs: Self) {
        self.low &= rhs.low;
        self.high &= rhs.high;
    }
}

impl Not for FrameLabelMask {
    type Output = Self;

    #[inline]
    fn not(self) -> Self::Output {
        Self {
            low: !self.low,
            high: !self.high,
        }
    }
}
