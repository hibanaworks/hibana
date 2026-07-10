use core::ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign, Not};

/// Choreography-facing message / branch identity.
///
/// This is intentionally crate-private. Application code expresses logical
/// labels through `g::Msg<L, P>` and observes them through `RouteBranch`.
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
    pub(crate) const fn new(raw: u8) -> Self {
        Self(raw)
    }

    #[inline]
    pub const fn raw(self) -> u8 {
        self.0
    }
}

/// Fixed mask over the complete `FrameLabel` domain.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct FrameLabelMask {
    limbs: [u8; 32],
}

impl FrameLabelMask {
    pub(crate) const EMPTY: Self = Self { limbs: [0; 32] };

    #[inline]
    pub(crate) const fn from_frame_label(frame_label: u8) -> Self {
        let mut limbs = [0u8; 32];
        limbs[(frame_label >> 3) as usize] = 1u8 << (frame_label & 7);
        Self { limbs }
    }

    #[inline]
    pub(crate) const fn contains_frame_label(self, frame_label: u8) -> bool {
        let limb = self.limbs[(frame_label >> 3) as usize];
        let bit = 1u8 << (frame_label & 7);
        (limb & bit) != 0
    }

    #[inline]
    pub(crate) const fn without(self, other: Self) -> Self {
        let mut limbs = [0u8; 32];
        let mut idx = 0usize;
        while idx < limbs.len() {
            limbs[idx] = self.limbs[idx] & !other.limbs[idx];
            idx += 1;
        }
        Self { limbs }
    }

    #[inline]
    pub(crate) fn insert_frame_label(&mut self, frame_label: u8) -> bool {
        let idx = (frame_label >> 3) as usize;
        let bit = 1u8 << (frame_label & 7);
        let before = self.limbs[idx];
        self.limbs[idx] = before | bit;
        before != self.limbs[idx]
    }
}

impl BitOr for FrameLabelMask {
    type Output = Self;

    #[inline]
    fn bitor(self, rhs: Self) -> Self::Output {
        let mut limbs = [0u8; 32];
        let mut idx = 0usize;
        while idx < limbs.len() {
            limbs[idx] = self.limbs[idx] | rhs.limbs[idx];
            idx += 1;
        }
        Self { limbs }
    }
}

impl BitOrAssign for FrameLabelMask {
    #[inline]
    fn bitor_assign(&mut self, rhs: Self) {
        let mut idx = 0usize;
        while idx < self.limbs.len() {
            self.limbs[idx] |= rhs.limbs[idx];
            idx += 1;
        }
    }
}

impl BitAnd for FrameLabelMask {
    type Output = Self;

    #[inline]
    fn bitand(self, rhs: Self) -> Self::Output {
        let mut limbs = [0u8; 32];
        let mut idx = 0usize;
        while idx < limbs.len() {
            limbs[idx] = self.limbs[idx] & rhs.limbs[idx];
            idx += 1;
        }
        Self { limbs }
    }
}

impl BitAndAssign for FrameLabelMask {
    #[inline]
    fn bitand_assign(&mut self, rhs: Self) {
        let mut idx = 0usize;
        while idx < self.limbs.len() {
            self.limbs[idx] &= rhs.limbs[idx];
            idx += 1;
        }
    }
}

impl Not for FrameLabelMask {
    type Output = Self;

    #[inline]
    fn not(self) -> Self::Output {
        let mut limbs = [0u8; 32];
        let mut idx = 0usize;
        while idx < limbs.len() {
            limbs[idx] = !self.limbs[idx];
            idx += 1;
        }
        Self { limbs }
    }
}
