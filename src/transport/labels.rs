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
    pub(crate) const fn intersects(self, other: Self) -> bool {
        let mut idx = 0usize;
        while idx < self.limbs.len() {
            if (self.limbs[idx] & other.limbs[idx]) != 0 {
                return true;
            }
            idx += 1;
        }
        false
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

    #[inline]
    pub(crate) fn remove_frame_label(&mut self, frame_label: u8) {
        let idx = (frame_label >> 3) as usize;
        let bit = 1u8 << (frame_label & 7);
        self.limbs[idx] &= !bit;
    }

    #[inline]
    fn next_limb_frame_label(limb_idx: usize, remaining: &mut u8) -> Option<u8> {
        if *remaining == 0 {
            return None;
        }
        let bit_idx = remaining.trailing_zeros() as u8;
        *remaining &= *remaining - 1;
        Some((limb_idx as u8) * 8 + bit_idx)
    }

    #[inline]
    fn take_matching_in_limb<F>(
        &mut self,
        limb_idx: usize,
        mut remaining: u8,
        matches: &mut F,
    ) -> Option<u8>
    where
        F: FnMut(u8) -> bool,
    {
        while let Some(frame_label) = Self::next_limb_frame_label(limb_idx, &mut remaining) {
            if matches(frame_label) {
                self.remove_frame_label(frame_label);
                return Some(frame_label);
            }
        }
        None
    }

    #[inline]
    pub(crate) fn take_matching<F>(&mut self, mut matches: F) -> Option<u8>
    where
        F: FnMut(u8) -> bool,
    {
        let mut idx = 0usize;
        while idx < self.limbs.len() {
            if let Some(frame_label) =
                self.take_matching_in_limb(idx, self.limbs[idx], &mut matches)
            {
                return Some(frame_label);
            }
            idx += 1;
        }
        None
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
