//! Type-level DSL for global and local session steps.
//!
//! Global protocols are described purely at the type level via typelists formed
//! from `SendStep<From, To, Msg>` nodes. Projection filters these typelists to
//! obtain role-local sequences that retain the underlying `MessageSpec`
//! metadata, enabling compile-time payload checking.

use core::marker::PhantomData;

use crate::global::{KnownRole, MessageSpec, ROLE_DOMAIN_SIZE, RoleMarker, SendableLabel};

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
}

/// Empty typelist.
pub struct StepNil;

/// Typelist cons node.
pub struct StepCons<Head, Tail>(PhantomData<(Head, Tail)>);

/// Global send step from `From` to `To` carrying message `Msg` on `LANE`.
///
/// The `LANE` parameter defaults to 0. When using `g::par`, different lanes allow
/// the same roles to communicate in parallel
/// without violating the disjoint constraint.
pub struct SendStep<From, To, Msg, const LANE: u8 = 0>(PhantomData<(From, To, Msg)>);

/// Typelist beginning with a control message send.
pub trait PolicyEligible {}

/// Local send transition (current role is the sender).
pub struct LocalSend<To, Msg>(PhantomData<(To, Msg)>);

/// Local receive transition (current role is the receiver).
pub struct LocalRecv<From, Msg>(PhantomData<(From, Msg)>);

/// Local action transition (self-send: sender == receiver).
pub struct LocalAction<Msg>(PhantomData<Msg>);

/// Sequence witness that preserves segment boundaries for substrate composition.
pub struct SeqSteps<Left, Right>(PhantomData<(Left, Right)>);

/// Route witness that preserves arm boundaries for source reconstruction.
pub struct RouteSteps<Left, Right>(PhantomData<(Left, Right)>);

/// Parallel witness that preserves arm boundaries for source reconstruction.
pub struct ParSteps<Left, Right>(PhantomData<(Left, Right)>);

/// Policy annotation witness for the final atom in a fragment.
pub struct PolicySteps<Inner, const POLICY_ID: u16>(PhantomData<Inner>);

impl Default for StepNil {
    fn default() -> Self {
        Self::new()
    }
}

impl StepNil {
    pub const fn new() -> Self {
        Self
    }
}

impl<Head, Tail> Default for StepCons<Head, Tail> {
    fn default() -> Self {
        Self::new()
    }
}

impl<Head, Tail> StepCons<Head, Tail> {
    pub const fn new() -> Self {
        Self(PhantomData)
    }
}

impl<Left, Right> Default for RouteSteps<Left, Right> {
    fn default() -> Self {
        Self::new()
    }
}

impl<Left, Right> RouteSteps<Left, Right> {
    pub const fn new() -> Self {
        Self(PhantomData)
    }
}

impl<Left, Right> Default for ParSteps<Left, Right> {
    fn default() -> Self {
        Self::new()
    }
}

impl<Left, Right> ParSteps<Left, Right> {
    pub const fn new() -> Self {
        Self(PhantomData)
    }
}

impl<Inner, const POLICY_ID: u16> Default for PolicySteps<Inner, POLICY_ID> {
    fn default() -> Self {
        Self::new()
    }
}

impl<Inner, const POLICY_ID: u16> PolicySteps<Inner, POLICY_ID> {
    pub const fn new() -> Self {
        Self(PhantomData)
    }
}

impl<From, To, Msg> Default for SendStep<From, To, Msg> {
    fn default() -> Self {
        Self::new()
    }
}

impl<From, To, Msg> SendStep<From, To, Msg> {
    pub const fn new() -> Self {
        Self(PhantomData)
    }
}

impl<To, Msg> Default for LocalSend<To, Msg> {
    fn default() -> Self {
        Self::new()
    }
}

impl<To, Msg> LocalSend<To, Msg> {
    pub const fn new() -> Self {
        Self(PhantomData)
    }
}

impl<From, Msg> Default for LocalRecv<From, Msg> {
    fn default() -> Self {
        Self::new()
    }
}

impl<From, Msg> LocalRecv<From, Msg> {
    pub const fn new() -> Self {
        Self(PhantomData)
    }
}

impl<From, To, Msg, const LANE: u8> PolicyEligible
    for StepCons<SendStep<From, To, Msg, LANE>, StepNil>
where
    From: KnownRole + RoleMarker,
    To: KnownRole + RoleMarker,
    Msg: MessageSpec + SendableLabel + crate::global::MessageControlSpec,
{
}

impl<Inner, const POLICY_ID: u16> PolicyEligible for PolicySteps<Inner, POLICY_ID> where
    Inner: PolicyEligible
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
