//! Compile-time support facts for the public `g` choreography witnesses.

use crate::global::ROLE_DOMAIN_SIZE;

// =============================================================================
// RoleLaneMask — compact role/lane facts for g::par disjoint checking
// =============================================================================

/// Const bitset of `(role, lane)` pairs for parallel composition checking.
///
/// This is the compiled summary used by `g::par`; the checker keeps the
/// correlation between role and lane without carrying a typelist-shaped owner
/// through the runtime path.
///
/// # Capacity
/// - Every `u8` lane.
/// - Maximum 16 roles per lane.
/// - Compile-time checking only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RoleLaneMask {
    words: [u64; ROLE_LANE_WORDS],
}

const ROLE_LANE_WORDS: usize = (ROLE_DOMAIN_SIZE * (u8::MAX as usize + 1)).div_ceil(64);

impl RoleLaneMask {
    /// Create an empty mask.
    pub const fn empty() -> Self {
        Self {
            words: [0; ROLE_LANE_WORDS],
        }
    }

    /// Add a role/lane fact.
    pub(crate) const fn with_role(mut self, role_index: u8, lane: u8) -> Self {
        assert!(
            (role_index as usize) < ROLE_DOMAIN_SIZE,
            "role_index must be < ROLE_DOMAIN_SIZE"
        );
        let bit_index = (lane as usize * ROLE_DOMAIN_SIZE) + role_index as usize;
        let word = bit_index / 64;
        let bit = 1u64 << (bit_index % 64);
        self.words[word] |= bit;
        self
    }

    /// Compute the union of two masks.
    pub const fn union(self, other: Self) -> Self {
        let mut out = Self::empty();
        let mut idx = 0usize;
        while idx < ROLE_LANE_WORDS {
            out.words[idx] = self.words[idx] | other.words[idx];
            idx += 1;
        }
        out
    }

    /// Check if any role overlaps within the same lane.
    pub const fn intersects(&self, other: &Self) -> bool {
        let mut idx = 0usize;
        while idx < ROLE_LANE_WORDS {
            if (self.words[idx] & other.words[idx]) != 0 {
                return true;
            }
            idx += 1;
        }
        false
    }

    /// Shift every lane fact by a projection-internal lane offset.
    pub(crate) const fn shift_lanes(self, offset: u16) -> Self {
        if offset == 0 {
            return self;
        }
        let mut out = Self::empty();
        let mut lane = 0usize;
        while lane <= u8::MAX as usize {
            let shifted = lane + offset as usize;
            let mut role = 0usize;
            while role < ROLE_DOMAIN_SIZE {
                let bit_index = (lane * ROLE_DOMAIN_SIZE) + role;
                let word = bit_index / 64;
                let bit = 1u64 << (bit_index % 64);
                if (self.words[word] & bit) != 0 {
                    if shifted > u8::MAX as usize {
                        panic!("projection internal lane overflow");
                    }
                    out = out.with_role(role as u8, shifted as u8);
                }
                role += 1;
            }
            lane += 1;
        }
        out
    }
}

/// Typelist beginning with a local loop controller decision send.
pub(crate) trait PolicyEligible {}

impl<const CONTROLLER: u8, const LOGICAL_LABEL: u8> PolicyEligible
    for crate::g::Send<
        CONTROLLER,
        CONTROLLER,
        crate::g::ControlMsg<LOGICAL_LABEL, crate::control::cap::resource_kinds::LoopContinueKind>,
    >
{
}

impl<const CONTROLLER: u8, const LOGICAL_LABEL: u8> PolicyEligible
    for crate::g::Send<
        CONTROLLER,
        CONTROLLER,
        crate::g::ControlMsg<LOGICAL_LABEL, crate::control::cap::resource_kinds::LoopBreakKind>,
    >
{
}

#[cfg(test)]
mod tests {
    use super::{ROLE_LANE_WORDS, RoleLaneMask};
    use core::mem::size_of;

    #[test]
    fn role_lane_mask_covers_every_u8_lane_as_fixed_bitset() {
        assert_eq!(
            size_of::<RoleLaneMask>(),
            ROLE_LANE_WORDS * size_of::<u64>(),
            "parallel ownership facts must stay as a fixed const bitset"
        );
    }

    #[test]
    fn role_lane_mask_tracks_same_role_same_lane_only() {
        let lane0_role0 = RoleLaneMask::empty().with_role(0, 0);
        let lane1_role0 = RoleLaneMask::empty().with_role(0, 1);
        let lane0_role1 = RoleLaneMask::empty().with_role(1, 0);

        assert!(!lane0_role0.intersects(&lane1_role0));
        assert!(!lane0_role0.intersects(&lane0_role1));
        assert!(lane0_role0.intersects(&RoleLaneMask::empty().with_role(0, 0)));
    }

    #[test]
    fn role_lane_mask_tracks_high_u8_lanes() {
        let high_role0 = RoleLaneMask::empty().with_role(0, u8::MAX);
        let high_role1 = RoleLaneMask::empty().with_role(1, u8::MAX);
        assert!(!high_role0.intersects(&high_role1));
        assert!(high_role0.intersects(&RoleLaneMask::empty().with_role(0, u8::MAX)));
    }
}
