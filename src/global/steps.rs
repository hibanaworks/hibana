//! Type-level DSL for global and local session steps.
//!
//! Global protocols are described purely at the type level via typelists formed
//! from `SendStep<From, To, Msg>` nodes. Projection filters these typelists to
//! obtain role-local sequences that retain the underlying `MessageSpec`
//! metadata, enabling compile-time payload checking.

use core::marker::PhantomData;

use super::const_dsl;
use crate::g::{KnownRole, Role};

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
/// - Maximum 8 Lanes (sufficient for HTTP/3 with Control, QPACK, Request, etc.)
/// - Maximum 32 Roles per Lane (same as the original `StepRoleSet::MASK`)
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
    /// Panics if `lane >= 8` or `role_index >= 32`.
    pub const fn with_role(mut self, role_index: u8, lane: u8) -> Self {
        assert!(lane < 8, "lane must be < 8");
        assert!(role_index < 32, "role_index must be < 32");
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

    /// Check if the set is empty (no roles in any lane).
    pub const fn is_empty(&self) -> bool {
        let mut i = 0;
        while i < 8 {
            if self.lanes[i] != 0 {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// Empty typelist.
pub struct StepNil;

/// Typelist cons node.
pub struct StepCons<Head, Tail>(PhantomData<(Head, Tail)>);

/// Global send step from `From` to `To` carrying message `Msg` on `LANE`.
///
/// The `LANE` parameter defaults to 0 for backward compatibility. When using
/// `g::par`, different Lanes allow the same roles to communicate in parallel
/// without violating the disjoint constraint.
pub struct SendStep<From, To, Msg, const LANE: u8 = 0>(PhantomData<(From, To, Msg)>);

/// Trait implemented by typelists that contain at least one step.
pub trait StepNonEmpty {}

/// Trait exposing whether a typelist is empty.
pub trait StepIsEmpty {
    const IS_EMPTY: bool;
}

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
pub trait ControlPlanEligible {}

/// Local send transition (current role is the sender).
pub struct LocalSend<To, Msg>(PhantomData<(To, Msg)>);

/// Local receive transition (current role is the receiver).
pub struct LocalRecv<From, Msg>(PhantomData<(From, Msg)>);

/// Local action transition (self-send: sender == receiver).
pub struct LocalAction<Msg>(PhantomData<Msg>);

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

/// Synthesize the `EffList` corresponding to a typelist of global steps.
pub trait BuildEffList {
    const EFF: const_dsl::EffList;
}

impl BuildEffList for StepNil {
    const EFF: const_dsl::EffList = const_dsl::EffList::new();
}

impl<From, To, Msg, const LANE: u8, Tail> BuildEffList for StepCons<SendStep<From, To, Msg, LANE>, Tail>
where
    From: KnownRole + crate::g::RoleMarker + RoleEq<To>,
    To: KnownRole + crate::g::RoleMarker,
    Msg: crate::g::MessageSpec + crate::g::SendableLabel + crate::global::MessageControlSpec,
    Tail: BuildEffList,
    // Enforce: CanonicalControl requires self-send (From == To)
    <Msg as crate::g::MessageSpec>::ControlKind:
        crate::global::RequireSelfSendForCanonical<<From as RoleEq<To>>::Output>,
{
    const EFF: const_dsl::EffList = {
        let head = const_dsl::const_send_typed::<From, To, Msg, LANE>();
        head.extend_list(Tail::EFF)
    };
}

/// Concatenate typelists.
pub trait StepConcat<Other> {
    type Output;
}

impl<Other> StepConcat<Other> for StepNil {
    type Output = Other;
}

impl<Head, Tail, Other> StepConcat<Other> for StepCons<Head, Tail>
where
    Tail: StepConcat<Other>,
{
    type Output = StepCons<Head, <Tail as StepConcat<Other>>::Output>;
}

impl StepIsEmpty for StepNil {
    const IS_EMPTY: bool = true;
}

impl<Head, Tail> StepIsEmpty for StepCons<Head, Tail> {
    const IS_EMPTY: bool = false;
}

impl<Head, Tail> StepNonEmpty for StepCons<Head, Tail> {}

impl StepRoleSet for StepNil {
    const ROLE_LANE_SET: RoleLaneSet = RoleLaneSet::empty();
}

impl<From, To, Msg, const LANE: u8, Tail> StepRoleSet for StepCons<SendStep<From, To, Msg, LANE>, Tail>
where
    From: KnownRole,
    To: KnownRole,
    Tail: StepRoleSet,
{
    const ROLE_LANE_SET: RoleLaneSet = Tail::ROLE_LANE_SET
        .with_role(From::INDEX, LANE)
        .with_role(To::INDEX, LANE);
}

impl<From, To, Msg, const LANE: u8> ControlPlanEligible for StepCons<SendStep<From, To, Msg, LANE>, StepNil>
where
    From: KnownRole + crate::g::RoleMarker,
    To: KnownRole + crate::g::RoleMarker,
    Msg: crate::g::MessageSpec + crate::g::SendableLabel + crate::global::MessageControlSpec,
{
}

/// Constraint ensuring a route arm begins with a self-send from the controller.
///
/// A route arm must start with `SendStep<Role<CONTROLLER>, Role<CONTROLLER>, Msg>`.
/// This enforces the hibana design where route decisions are made via local
/// self-send control messages processed by `flow().send()`.
///
/// Other roles discover the selected arm via resolver or [`poll_route_decision`].
///
/// [`poll_route_decision`]: crate::endpoint::CursorEndpoint::poll_route_decision
pub trait RouteArm<const CONTROLLER: u8> {
    /// Message dispatched by the controller at the beginning of the arm (self-send).
    type Msg: crate::g::MessageSpec + crate::g::SendableLabel;
    /// Remaining steps after the initial controller send.
    type Tail;
}

// Implementation: route arm must be self-send (CONTROLLER → CONTROLLER)
// Note: const LANE is added for completeness but route arms are typically on Lane 0
impl<const CONTROLLER: u8, Msg, const LANE: u8, Tail> RouteArm<CONTROLLER>
    for StepCons<SendStep<Role<CONTROLLER>, Role<CONTROLLER>, Msg, LANE>, Tail>
where
    Msg: crate::g::MessageSpec + crate::g::SendableLabel,
{
    type Msg = Msg;
    type Tail = Tail;
}

/// Type-level booleans used during projection.
pub trait Bool {}
pub struct True;
pub struct False;
impl Bool for True {}
impl Bool for False {}

/// Role equality at the type level.
pub trait RoleEq<Other> {
    type Output: Bool;
}

/// Marker trait for self-equality (From == To).
/// This is automatically true for `Role<N>: RoleEq<Role<N>>`.
pub trait IsSelfSend {}

macro_rules! impl_role_eq {
    () => {};
    ($head:literal $(,$tail:literal)*) => {
        impl RoleEq<Role<$head>> for Role<$head> {
            type Output = True;
        }
        impl IsSelfSend for Role<$head> {}
        $(
            impl RoleEq<Role<$tail>> for Role<$head> {
                type Output = False;
            }
            impl RoleEq<Role<$head>> for Role<$tail> {
                type Output = False;
            }
        )*
        impl_role_eq!($($tail),*);
    };
}

impl_role_eq!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15);

/// Selection logic for a single send step.
pub trait SelectLocal<SendFlag: Bool, RecvFlag: Bool, Local, From, To, Msg> {
    type Output;
}

impl<Local, From, To, Msg> SelectLocal<True, False, Local, From, To, Msg> for ()
where
    To: KnownRole,
    Msg: crate::g::MessageSpec,
{
    type Output = StepCons<LocalSend<To, Msg>, StepNil>;
}

impl<Local, From, To, Msg> SelectLocal<False, True, Local, From, To, Msg> for ()
where
    From: KnownRole,
    Msg: crate::g::MessageSpec,
{
    type Output = StepCons<LocalRecv<From, Msg>, StepNil>;
}

impl<Local, From, To, Msg> SelectLocal<False, False, Local, From, To, Msg> for ()
where
    Msg: crate::g::MessageSpec,
{
    type Output = StepNil;
}

impl<Local, From, To, Msg> SelectLocal<True, True, Local, From, To, Msg> for ()
where
    Msg: crate::g::MessageSpec,
{
    type Output = StepCons<LocalAction<Msg>, StepNil>;
}

/// Project a global typelist to the local steps for `Local`.
pub trait ProjectRole<Local> {
    type Output;
}

impl<Local> ProjectRole<Local> for StepNil {
    type Output = StepNil;
}

impl<Local, From, To, Msg, const LANE: u8, Tail> ProjectRole<Local> for StepCons<SendStep<From, To, Msg, LANE>, Tail>
where
    Local: KnownRole,
    From: KnownRole + RoleEq<Local>,
    To: KnownRole + RoleEq<Local>,
    Msg: crate::g::MessageSpec,
    Tail: ProjectRole<Local>,
    (): SelectLocal<
            <From as RoleEq<Local>>::Output,
            <To as RoleEq<Local>>::Output,
            Local,
            From,
            To,
            Msg,
        >,
    <() as SelectLocal<
        <From as RoleEq<Local>>::Output,
        <To as RoleEq<Local>>::Output,
        Local,
        From,
        To,
        Msg,
    >>::Output: StepConcat<<Tail as ProjectRole<Local>>::Output>,
{
    type Output = <<() as SelectLocal<
        <From as RoleEq<Local>>::Output,
        <To as RoleEq<Local>>::Output,
        Local,
        From,
        To,
        Msg,
    >>::Output as StepConcat<<Tail as ProjectRole<Local>>::Output>>::Output;
}
