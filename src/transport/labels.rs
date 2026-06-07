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
    word0: u64,
    word1: u64,
    word2: u64,
    word3: u64,
}

impl FrameLabelMask {
    pub(crate) const EMPTY: Self = Self {
        word0: 0,
        word1: 0,
        word2: 0,
        word3: 0,
    };

    #[inline]
    pub(crate) const fn from_frame_label(frame_label: u8) -> Self {
        let bit = 1u64 << ((frame_label & 63) as u32);
        match frame_label >> 6 {
            0 => Self {
                word0: bit,
                ..Self::EMPTY
            },
            1 => Self {
                word1: bit,
                ..Self::EMPTY
            },
            2 => Self {
                word2: bit,
                ..Self::EMPTY
            },
            _ => Self {
                word3: bit,
                ..Self::EMPTY
            },
        }
    }

    #[inline]
    pub(crate) const fn contains_frame_label(self, frame_label: u8) -> bool {
        let bit = 1u64 << ((frame_label & 63) as u32);
        match frame_label >> 6 {
            0 => (self.word0 & bit) != 0,
            1 => (self.word1 & bit) != 0,
            2 => (self.word2 & bit) != 0,
            _ => (self.word3 & bit) != 0,
        }
    }

    #[inline]
    pub(crate) const fn intersects(self, other: Self) -> bool {
        (self.word0 & other.word0) != 0
            || (self.word1 & other.word1) != 0
            || (self.word2 & other.word2) != 0
            || (self.word3 & other.word3) != 0
    }

    #[inline]
    pub(crate) const fn without(self, other: Self) -> Self {
        Self {
            word0: self.word0 & !other.word0,
            word1: self.word1 & !other.word1,
            word2: self.word2 & !other.word2,
            word3: self.word3 & !other.word3,
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
    fn next_word_frame_label(word_idx: usize, remaining: &mut u64) -> Option<u8> {
        if *remaining == 0 {
            return None;
        }
        let bit_idx = remaining.trailing_zeros() as u8;
        *remaining &= *remaining - 1;
        Some((word_idx as u8) * 64 + bit_idx)
    }

    #[cfg(test)]
    #[inline]
    fn has_matching_in_word<F>(word_idx: usize, mut remaining: u64, matches: &mut F) -> bool
    where
        F: FnMut(u8) -> bool,
    {
        while let Some(frame_label) = Self::next_word_frame_label(word_idx, &mut remaining) {
            if matches(frame_label) {
                return true;
            }
        }
        false
    }

    #[inline]
    fn take_matching_in_word<F>(
        &mut self,
        word_idx: usize,
        mut remaining: u64,
        matches: &mut F,
    ) -> Option<u8>
    where
        F: FnMut(u8) -> bool,
    {
        while let Some(frame_label) = Self::next_word_frame_label(word_idx, &mut remaining) {
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
        if let Some(frame_label) = self.take_matching_in_word(0, self.word0, &mut matches) {
            return Some(frame_label);
        }
        if let Some(frame_label) = self.take_matching_in_word(1, self.word1, &mut matches) {
            return Some(frame_label);
        }
        if let Some(frame_label) = self.take_matching_in_word(2, self.word2, &mut matches) {
            return Some(frame_label);
        }
        self.take_matching_in_word(3, self.word3, &mut matches)
    }

    #[cfg(test)]
    pub(crate) fn has_matching<F>(self, mut matches: F) -> bool
    where
        F: FnMut(u8) -> bool,
    {
        Self::has_matching_in_word(0, self.word0, &mut matches)
            || Self::has_matching_in_word(1, self.word1, &mut matches)
            || Self::has_matching_in_word(2, self.word2, &mut matches)
            || Self::has_matching_in_word(3, self.word3, &mut matches)
    }
}

impl BitOr for FrameLabelMask {
    type Output = Self;

    #[inline]
    fn bitor(self, rhs: Self) -> Self::Output {
        Self {
            word0: self.word0 | rhs.word0,
            word1: self.word1 | rhs.word1,
            word2: self.word2 | rhs.word2,
            word3: self.word3 | rhs.word3,
        }
    }
}

impl BitOrAssign for FrameLabelMask {
    #[inline]
    fn bitor_assign(&mut self, rhs: Self) {
        self.word0 |= rhs.word0;
        self.word1 |= rhs.word1;
        self.word2 |= rhs.word2;
        self.word3 |= rhs.word3;
    }
}

impl BitAnd for FrameLabelMask {
    type Output = Self;

    #[inline]
    fn bitand(self, rhs: Self) -> Self::Output {
        Self {
            word0: self.word0 & rhs.word0,
            word1: self.word1 & rhs.word1,
            word2: self.word2 & rhs.word2,
            word3: self.word3 & rhs.word3,
        }
    }
}

impl BitAndAssign for FrameLabelMask {
    #[inline]
    fn bitand_assign(&mut self, rhs: Self) {
        self.word0 &= rhs.word0;
        self.word1 &= rhs.word1;
        self.word2 &= rhs.word2;
        self.word3 &= rhs.word3;
    }
}

impl Not for FrameLabelMask {
    type Output = Self;

    #[inline]
    fn not(self) -> Self::Output {
        Self {
            word0: !self.word0,
            word1: !self.word1,
            word2: !self.word2,
            word3: !self.word3,
        }
    }
}
