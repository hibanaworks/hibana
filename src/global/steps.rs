//! Compile-time support facts for the public `g` choreography witnesses.

use crate::global::ROLE_DOMAIN_SIZE;

// =============================================================================
// RoleLaneMask — compact role/lane facts for g::par disjoint checking
// =============================================================================

/// Lane-indexed role facts for parallel composition checking.
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
    lanes: [u16; ROLE_LANE_COUNT],
}

const ROLE_LANE_COUNT: usize = u8::MAX as usize + 1;

impl RoleLaneMask {
    /// Create an empty mask.
    pub(crate) const fn empty() -> Self {
        Self {
            lanes: [0; ROLE_LANE_COUNT],
        }
    }

    /// Add a role/lane fact.
    pub(crate) const fn with_role(mut self, role_index: u8, lane: u8) -> Self {
        assert!(
            (role_index as usize) < ROLE_DOMAIN_SIZE,
            "role_index must be < ROLE_DOMAIN_SIZE"
        );
        let bit = 1u16 << role_index;
        self.lanes[lane as usize] |= bit;
        self
    }

    /// Compute the union of two masks inside the caller-owned active lane span.
    pub(crate) const fn union(self, other: Self, active_span: u16) -> Self {
        let mut out = Self::empty();
        let mut lane = 0usize;
        while lane < active_span as usize {
            out.lanes[lane] = self.lanes[lane] | other.lanes[lane];
            lane += 1;
        }
        out
    }

    /// Check if any role overlaps within the caller-owned active lane span.
    pub(crate) const fn intersects(&self, other: &Self, active_span: u16) -> bool {
        let mut lane = 0usize;
        while lane < active_span as usize {
            if (self.lanes[lane] & other.lanes[lane]) != 0 {
                return true;
            }
            lane += 1;
        }
        false
    }

    /// Shift every lane fact by a projection-internal lane offset.
    pub(crate) const fn shift_lanes(self, offset: u16, active_span: u16) -> Self {
        if offset == 0 {
            return self;
        }
        let mut out = Self::empty();
        let mut lane = 0usize;
        while lane < active_span as usize {
            let shifted = lane + offset as usize;
            let role_bits = self.lanes[lane];
            if role_bits != 0 {
                if shifted > u8::MAX as usize {
                    panic!("projection internal lane overflow");
                }
                out.lanes[shifted] = role_bits;
            }
            lane += 1;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::{ROLE_LANE_COUNT, RoleLaneMask};
    use core::mem::size_of;

    #[test]
    fn role_lane_mask_is_lane_indexed_u16_storage() {
        assert_eq!(
            size_of::<RoleLaneMask>(),
            ROLE_LANE_COUNT * size_of::<u16>(),
            "parallel ownership facts must stay lane-indexed without u64 words"
        );
    }

    #[test]
    fn role_lane_mask_tracks_same_role_same_lane_only() {
        let lane0_role0 = RoleLaneMask::empty().with_role(0, 0);
        let lane1_role0 = RoleLaneMask::empty().with_role(0, 1);
        let lane0_role1 = RoleLaneMask::empty().with_role(1, 0);

        assert!(!lane0_role0.intersects(&lane1_role0, 2));
        assert!(!lane0_role0.intersects(&lane0_role1, 1));
        assert!(lane0_role0.intersects(&RoleLaneMask::empty().with_role(0, 0), 1));
    }

    #[test]
    fn role_lane_mask_tracks_high_u8_lanes() {
        let high_role0 = RoleLaneMask::empty().with_role(0, u8::MAX);
        let high_role1 = RoleLaneMask::empty().with_role(1, u8::MAX);
        assert!(!high_role0.intersects(&high_role1, 256));
        assert!(high_role0.intersects(&RoleLaneMask::empty().with_role(0, u8::MAX), 256));
    }

    #[test]
    fn role_lane_mask_active_span_controls_high_lane_collision() {
        let high = RoleLaneMask::empty().with_role(15, u8::MAX);
        assert!(!high.intersects(&high, 255));
        assert!(high.intersects(&high, 256));
    }
}
