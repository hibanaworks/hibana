//! Type-level DSL for global and local session steps.
//!
//! Global protocols are described purely at the type level via typelists formed
//! from `SendStep<From, To, Msg>` nodes. Projection filters these typelists to
//! obtain role-local sequences that retain the underlying `MessageSpec`
//! metadata, enabling compile-time payload checking.

use core::marker::PhantomData;

use super::const_dsl;
use super::program::ProgramSource;
use crate::global::{KnownRole, MessageSpec, Role, RoleMarker, SendableLabel};

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
    pub(crate) const fn with_role(mut self, role_index: u8, lane: u8) -> Self {
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

/// Loop continue arm with a controller self-send head.
pub type LoopContinueSteps<Controller, ContMsg, Tail = StepNil> =
    SeqSteps<StepCons<SendStep<Controller, Controller, ContMsg>, StepNil>, Tail>;

/// Loop break arm with a controller self-send head.
pub type LoopBreakSteps<Controller, BreakMsg, Tail = StepNil> =
    StepCons<SendStep<Controller, Controller, BreakMsg>, Tail>;

/// Lane-qualified loop continue arm with a controller self-send head.
pub type LoopContinueStepsL<Controller, ContMsg, const LANE: u8, Tail = StepNil> =
    SeqSteps<StepCons<SendStep<Controller, Controller, ContMsg, LANE>, StepNil>, Tail>;

/// Lane-qualified loop break arm with a controller self-send head.
pub type LoopBreakStepsL<Controller, BreakMsg, const LANE: u8, Tail = StepNil> =
    StepCons<SendStep<Controller, Controller, BreakMsg, LANE>, Tail>;

/// Binary loop decision witness composed from continue and break arms.
pub type LoopDecisionSteps<Controller, ContMsg, BreakMsg, BreakTail = StepNil, ContTail = StepNil> =
    <LoopContinueSteps<Controller, ContMsg, ContTail> as StepConcat<
        LoopBreakSteps<Controller, BreakMsg, BreakTail>,
    >>::Output;

/// Lane-qualified binary loop decision witness.
pub type LoopDecisionStepsL<
    Controller,
    ContMsg,
    BreakMsg,
    const LANE: u8,
    BreakTail = StepNil,
    ContTail = StepNil,
> = <LoopContinueStepsL<Controller, ContMsg, LANE, ContTail> as StepConcat<
    LoopBreakStepsL<Controller, BreakMsg, LANE, BreakTail>,
>>::Output;

/// Canonical loop witness that preserves the body segment in the continue arm.
pub type LoopSteps<
    BodySteps,
    Controller,
    ContMsg,
    BreakMsg,
    BreakTail = StepNil,
    ContTail = StepNil,
> = LoopDecisionSteps<
    Controller,
    ContMsg,
    BreakMsg,
    BreakTail,
    <BodySteps as StepConcat<ContTail>>::Output,
>;

/// Lane-qualified canonical loop witness that preserves the body segment.
pub type LoopStepsL<
    BodySteps,
    Controller,
    ContMsg,
    BreakMsg,
    const LANE: u8,
    BreakTail = StepNil,
    ContTail = StepNil,
> = LoopDecisionStepsL<
    Controller,
    ContMsg,
    BreakMsg,
    LANE,
    BreakTail,
    <BodySteps as StepConcat<ContTail>>::Output,
>;

impl Default for StepNil {
    fn default() -> Self {
        Self::new()
    }
}

impl StepNil {
    /// Canonical zero-fragment program witness for substrate-side composition.
    pub const PROGRAM: ProgramSource<Self> = ProgramSource::<Self>::empty();

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

impl<From, To, Msg, const LANE: u8, Tail> BuildEffList
    for StepCons<SendStep<From, To, Msg, LANE>, Tail>
where
    From: KnownRole + RoleMarker + RoleEq<To>,
    To: KnownRole + RoleMarker,
    Msg: MessageSpec + SendableLabel + crate::global::MessageControlSpec,
    Tail: BuildEffList,
    // Enforce: CanonicalControl requires self-send (From == To)
    <Msg as MessageSpec>::ControlKind:
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

impl<Left, Right, Other> StepConcat<Other> for SeqSteps<Left, Right>
where
    Right: StepConcat<Other>,
{
    type Output = SeqSteps<Left, <Right as StepConcat<Other>>::Output>;
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

impl<From, To, Msg, const LANE: u8> PolicyEligible
    for StepCons<SendStep<From, To, Msg, LANE>, StepNil>
where
    From: KnownRole + RoleMarker,
    To: KnownRole + RoleMarker,
    Msg: MessageSpec + SendableLabel + crate::global::MessageControlSpec,
{
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

macro_rules! impl_role_eq {
    () => {};
    ($head:literal $(,$tail:literal)*) => {
        impl RoleEq<Role<$head>> for Role<$head> {
            type Output = True;
        }
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
    Msg: MessageSpec,
{
    type Output = StepCons<LocalSend<To, Msg>, StepNil>;
}

impl<Local, From, To, Msg> SelectLocal<False, True, Local, From, To, Msg> for ()
where
    From: KnownRole,
    Msg: MessageSpec,
{
    type Output = StepCons<LocalRecv<From, Msg>, StepNil>;
}

impl<Local, From, To, Msg> SelectLocal<False, False, Local, From, To, Msg> for ()
where
    Msg: MessageSpec,
{
    type Output = StepNil;
}

impl<Local, From, To, Msg> SelectLocal<True, True, Local, From, To, Msg> for ()
where
    Msg: MessageSpec,
{
    type Output = StepCons<LocalAction<Msg>, StepNil>;
}

/// Project a global typelist to the local steps for `Local`.
pub trait ProjectRole<Local> {
    type Output: StepCount;
}

pub trait StepCount {
    const LEN: usize;
}

impl<Local> ProjectRole<Local> for StepNil {
    type Output = StepNil;
}

impl StepCount for StepNil {
    const LEN: usize = 0;
}

impl<Local, From, To, Msg, const LANE: u8, Tail> ProjectRole<Local>
    for StepCons<SendStep<From, To, Msg, LANE>, Tail>
where
    Local: KnownRole,
    From: KnownRole + RoleEq<Local>,
    To: KnownRole + RoleEq<Local>,
    Msg: MessageSpec,
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
    <<() as SelectLocal<
        <From as RoleEq<Local>>::Output,
        <To as RoleEq<Local>>::Output,
        Local,
        From,
        To,
        Msg,
    >>::Output as StepConcat<<Tail as ProjectRole<Local>>::Output>>::Output: StepCount,
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

impl<Head, Tail> StepCount for StepCons<Head, Tail>
where
    Tail: StepCount,
{
    const LEN: usize = 1 + Tail::LEN;
}

impl<Local, Left, Right> ProjectRole<Local> for SeqSteps<Left, Right>
where
    Left: ProjectRole<Local>,
    Right: ProjectRole<Local>,
    <Left as ProjectRole<Local>>::Output: StepConcat<<Right as ProjectRole<Local>>::Output>,
    <<Left as ProjectRole<Local>>::Output as StepConcat<
        <Right as ProjectRole<Local>>::Output,
    >>::Output: StepCount,
{
    type Output = <<Left as ProjectRole<Local>>::Output as StepConcat<
        <Right as ProjectRole<Local>>::Output,
    >>::Output;
}

impl<Left, Right> StepCount for SeqSteps<Left, Right>
where
    Left: StepCount,
    Right: StepCount,
{
    const LEN: usize = Left::LEN + Right::LEN;
}
