//! Type-level DSL for global and local session steps.
//!
//! Global protocols are described purely at the type level via typelists formed
//! from `SendStep<From, To, Msg>` nodes. Projection filters these typelists to
//! obtain role-local sequences that retain the underlying `MessageSpec`
//! metadata, enabling compile-time payload checking.

use core::marker::PhantomData;

use crate::global::{KnownRole, MessageSpec, RoleMarker, SendableLabel};

// =============================================================================
// RoleLaneSet — Lane-aware role set for g::par disjoint checking
// =============================================================================

/// Lane-indexed role set for parallel composition disjoint checking.
///
/// Maintains the correlation between (role, lane) pairs to enable
/// Lane-aware disjoint verification in `g::par`. From an AMPST perspective,
/// different Lanes represent independent channels, so the same roles can
/// communicate in parallel on different Lanes without violating linearity.
///
/// # Capacity
/// - Maximum 8 Lanes (sufficient for layered control/data parallelism)
/// - Maximum 16 Roles per Lane (matches the public `Role<IDX>` contract)
/// - Copy: 32 bytes (compile-time checking only, zero runtime cost)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoleLaneSet {
    /// Each element is a 32-bit role mask for that Lane index.
    /// lanes[0] = Lane 0's role mask, lanes[1] = Lane 1's role mask, etc.
    lanes: [u32; 8],
}

impl RoleLaneSet {
    /// Create an empty set with no roles in any lane.
    pub const fn empty() -> Self {
        Self { lanes: [0; 8] }
    }

    /// Add a role to a specific lane.
    ///
    /// # Panics
    /// Panics if `lane >= 8` or `role_index >= 16`.
    pub(crate) const fn with_role(mut self, role_index: u8, lane: u8) -> Self {
        assert!(lane < 8, "lane must be < 8");
        assert!(role_index < 16, "role_index must be < 16");
        self.lanes[lane as usize] |= 1u32 << role_index;
        self
    }

    /// Compute the union of two role-lane sets.
    pub const fn union(self, other: Self) -> Self {
        Self {
            lanes: [
                self.lanes[0] | other.lanes[0],
                self.lanes[1] | other.lanes[1],
                self.lanes[2] | other.lanes[2],
                self.lanes[3] | other.lanes[3],
                self.lanes[4] | other.lanes[4],
                self.lanes[5] | other.lanes[5],
                self.lanes[6] | other.lanes[6],
                self.lanes[7] | other.lanes[7],
            ],
        }
    }

    /// Check if any role overlaps within the same lane.
    ///
    /// Returns `true` if there exists at least one lane where both sets
    /// have a common role (i.e., `self.lanes[i] & other.lanes[i] != 0`).
    pub const fn intersects(&self, other: &Self) -> bool {
        let mut i = 0;
        while i < 8 {
            if (self.lanes[i] & other.lanes[i]) != 0 {
                return true;
            }
            i += 1;
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

/// Trait exposing the set of (role, lane) pairs participating in a typelist.
///
/// Used by `g::par` to verify that parallel lanes use disjoint (role, lane) pairs.
/// From an AMPST perspective, different Lanes are independent channels, so the
/// same roles can communicate in parallel on different Lanes.
pub trait StepRoleSet {
    /// The set of (role, lane) pairs in this typelist.
    const ROLE_LANE_SET: RoleLaneSet;
}

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

/// Loop continue arm with a controller self-send head.
#[cfg(test)]
pub type LoopContinueSteps<Controller, ContMsg, Tail = StepNil> =
    SeqSteps<StepCons<SendStep<Controller, Controller, ContMsg>, StepNil>, Tail>;

/// Loop break arm with a controller self-send head.
#[cfg(test)]
pub type LoopBreakSteps<Controller, BreakMsg, Tail = StepNil> =
    StepCons<SendStep<Controller, Controller, BreakMsg>, Tail>;

/// Binary loop decision witness composed from continue and break arms.
#[cfg(test)]
pub type LoopDecisionSteps<Controller, ContMsg, BreakMsg, BreakTail = StepNil, ContTail = StepNil> =
    RouteSteps<
        LoopContinueSteps<Controller, ContMsg, ContTail>,
        LoopBreakSteps<Controller, BreakMsg, BreakTail>,
    >;

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

impl StepRoleSet for StepNil {
    const ROLE_LANE_SET: RoleLaneSet = RoleLaneSet::empty();
}

impl<From, To, Msg, const LANE: u8, Tail> StepRoleSet
    for StepCons<SendStep<From, To, Msg, LANE>, Tail>
where
    From: KnownRole,
    To: KnownRole,
    Tail: StepRoleSet,
{
    const ROLE_LANE_SET: RoleLaneSet = Tail::ROLE_LANE_SET
        .with_role(From::INDEX, LANE)
        .with_role(To::INDEX, LANE);
}

impl<Left, Right> StepRoleSet for SeqSteps<Left, Right>
where
    Left: StepRoleSet,
    Right: StepRoleSet,
{
    const ROLE_LANE_SET: RoleLaneSet = Left::ROLE_LANE_SET.union(Right::ROLE_LANE_SET);
}

impl<Left, Right> StepRoleSet for RouteSteps<Left, Right>
where
    Left: StepRoleSet,
    Right: StepRoleSet,
{
    const ROLE_LANE_SET: RoleLaneSet = Left::ROLE_LANE_SET.union(Right::ROLE_LANE_SET);
}

impl<Left, Right> StepRoleSet for ParSteps<Left, Right>
where
    Left: StepRoleSet,
    Right: StepRoleSet,
{
    const ROLE_LANE_SET: RoleLaneSet = Left::ROLE_LANE_SET.union(Right::ROLE_LANE_SET);
}

impl<Inner, const POLICY_ID: u16> StepRoleSet for PolicySteps<Inner, POLICY_ID>
where
    Inner: StepRoleSet,
{
    const ROLE_LANE_SET: RoleLaneSet = Inner::ROLE_LANE_SET;
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
