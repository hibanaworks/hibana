//! Typed program representation built from const DSL combinators.
//!
//! `Program<Steps>` is the public choreography owner consumed by projection and
//! attach paths. The raw `EffList` source and cheap composition hints stay
//! crate-private behind type-level source builders.

use core::marker::PhantomData;

use crate::global::compiled::lowering::{LoweringSummary, validate_all_roles};
use crate::global::const_dsl::{EffList, PolicyMode, ScopeId};
use crate::global::steps::{
    LocalAction, LocalRecv, LocalSend, ParSteps, PolicyEligible, PolicySteps, RoleLaneMask,
    RouteSteps, SendStep, SeqSteps, StepCons, StepNil,
};
use crate::global::{
    LoopControlMeaning, NonEmptyParallelArm, RouteArmHead, RouteArmLoopHead,
    SameRouteControllerRole, TailLoopControl, assert_distinct_route_labels,
};

/// Neutral program-level facts emitted by projection metadata visitors.
///
/// These facts describe the projected hibana program shape only. They do not
/// name WASI, boards, sites, or any downstream runtime concept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProjectionProgramFacts {
    pub role_count: u8,
    pub eff_count: u16,
    pub scope_count: u16,
    pub route_scope_count: u16,
    pub parallel_enter_count: u16,
    pub control_scope_mask: u8,
    pub fingerprint: [u64; 2],
}

/// Neutral atom facts emitted by projection metadata visitors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProjectionAtomSpec {
    pub eff_index: u16,
    pub from: u8,
    pub to: u8,
    pub label: u8,
    pub lane: u8,
    pub is_control: bool,
    pub resource: Option<u8>,
    pub control_scope: Option<u8>,
    pub control_path: Option<u8>,
    pub control_shot: Option<u8>,
    pub control_op: Option<u8>,
    pub control_tap_id: Option<u16>,
    pub control_auto_mint_wire: bool,
}

/// Stable-within-build fingerprint for Rust type-level projection metadata.
///
/// The value is derived from Rust's type name and is intentionally neutral: it
/// does not name WASI, boards, sites, or downstream runtime concepts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProjectionTypeFingerprint {
    pub words: [u64; 2],
}

impl ProjectionTypeFingerprint {
    const SEED0: u64 = 0xcbf2_9ce4_8422_2325;
    const SEED1: u64 = 0x8422_2325_cbf2_9ce4;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    pub fn of<T: ?Sized>() -> Self {
        Self::from_type_name(core::any::type_name::<T>())
    }

    pub fn from_type_name(name: &str) -> Self {
        let bytes = name.as_bytes();
        let mut left = Self::SEED0;
        let mut right = Self::SEED1;
        let mut idx = 0usize;
        while idx < bytes.len() {
            let byte = bytes[idx] as u64;
            left ^= byte;
            left = left.wrapping_mul(Self::PRIME);
            right ^= byte.rotate_left((idx % 8) as u32);
            right = right.wrapping_mul(Self::PRIME.rotate_left(13));
            idx += 1;
        }
        Self {
            words: [left, right],
        }
    }
}

/// Neutral typed-message facts emitted by projection metadata visitors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProjectionMessageSpec {
    pub eff_index: u16,
    pub from: u8,
    pub to: u8,
    pub label: u8,
    pub lane: u8,
    pub is_control: bool,
    pub message_type: ProjectionTypeFingerprint,
    pub payload_type: ProjectionTypeFingerprint,
    pub control_type: ProjectionTypeFingerprint,
}

/// Neutral policy facts emitted by projection metadata visitors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProjectionPolicySpec {
    pub eff_index: u16,
    pub policy_id: u16,
    pub scope_raw: u64,
}

/// Neutral scope facts emitted by projection metadata visitors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProjectionScopeSpec {
    pub offset: u16,
    pub scope_raw: u64,
    pub scope_kind: u8,
    pub event: u8,
    pub linger: bool,
    pub controller_role: Option<u8>,
}

/// Visitor for neutral projection metadata.
///
/// Downstream crates should derive capacity from these official projection
/// facts instead of deriving meaning from helper names, labels as strings, or
/// an appkit-specific choreography wrapper.
pub trait ProjectionMetadataVisitor {
    fn visit_program(&mut self, _: ProjectionProgramFacts) {}

    fn visit_atom(&mut self, _: ProjectionAtomSpec) {}

    fn visit_message(&mut self, _: ProjectionMessageSpec) {}

    fn visit_policy(&mut self, _: ProjectionPolicySpec) {}

    fn visit_scope(&mut self, _: ProjectionScopeSpec) {}
}

/// Public marker for raw hibana programs that can be projected and visited.
pub trait Projectable<Universe> {
    fn visit_projection_metadata<V: ProjectionMetadataVisitor>(&self, visitor: &mut V);

    fn project<const ROLE: u8>(&self) -> crate::global::role_program::RoleProgram<ROLE>;
}

#[derive(Clone, Copy)]
pub(crate) struct ProgramSourceData {
    eff: EffList,
    role_lane_mask: RoleLaneMask,
    loop_scope_pending: bool,
    tail_is_loop_control: bool,
}

impl ProgramSourceData {
    pub(crate) const fn empty() -> Self {
        Self::from_parts(EffList::new(), RoleLaneMask::empty(), false, false)
    }

    const fn from_parts(
        eff: EffList,
        role_lane_mask: RoleLaneMask,
        loop_scope_pending: bool,
        tail_is_loop_control: bool,
    ) -> Self {
        Self {
            eff,
            role_lane_mask,
            loop_scope_pending,
            tail_is_loop_control,
        }
    }

    #[inline(always)]
    pub(crate) const fn eff_list(&self) -> &EffList {
        &self.eff
    }

    #[inline(always)]
    const fn scope_budget(&self) -> u16 {
        self.eff.scope_budget()
    }

    #[inline(always)]
    const fn into_eff(self) -> EffList {
        self.eff
    }

    #[inline(always)]
    pub(crate) const fn role_lane_mask(&self) -> RoleLaneMask {
        self.role_lane_mask
    }

    const fn seq(self, next: Self) -> Self {
        let next_tail_is_loop_control = if next.eff.is_empty() {
            self.tail_is_loop_control
        } else {
            next.tail_is_loop_control
        };
        let rebased = next.eff.rebase_scopes(self.scope_budget());
        let mut eff = self.eff;
        let scope_budget = self.scope_budget();
        if next.loop_scope_pending {
            if eff.is_empty() {
                panic!("loop body must contain at least one step");
            }
            let loop_scope =
                ScopeId::loop_scope(add_scope_budget(scope_budget, next.scope_budget()));
            let scoped_next = rebased.with_scope(loop_scope);
            eff = if self.tail_is_loop_control {
                eff.with_scope(loop_scope).extend_list(scoped_next)
            } else {
                eff.extend_list(scoped_next)
            };
            add_scope_budget(scope_budget, add_scope_budget(next.scope_budget(), 1));
        } else {
            eff = eff.extend_list(rebased);
            add_scope_budget(scope_budget, next.scope_budget());
        }
        Self::from_parts(
            eff,
            self.role_lane_mask.union(next.role_lane_mask),
            false,
            next_tail_is_loop_control,
        )
    }

    const fn with_policy(self, policy_id: u16) -> Self {
        Self::from_parts(
            self.eff.with_policy(PolicyMode::dynamic(policy_id)),
            self.role_lane_mask,
            self.loop_scope_pending,
            self.tail_is_loop_control,
        )
    }

    const fn route_with_controller(self, right: Self, controller: u8, is_loop: bool) -> Self {
        let scope = ScopeId::route(0);
        let left_budget = self.scope_budget();
        let left_arm = self.into_eff();
        let right_arm = right.into_eff();
        let right_offset = add_scope_budget(1, left_budget);
        let left_eff = left_arm
            .rebase_scopes(1)
            .with_scope_controller(scope, controller);
        let right_eff = right_arm
            .rebase_scopes(right_offset)
            .with_scope(scope)
            .with_scope_controller_role(scope, controller);
        let eff = left_eff.extend_list(right_eff);
        let eff = if is_loop {
            eff.with_scope_linger(scope, true)
        } else {
            eff
        };
        let loop_scope_pending = eff.scope_has_linger(scope);
        Self::from_parts(
            eff,
            self.role_lane_mask.union(right.role_lane_mask),
            loop_scope_pending,
            right.tail_is_loop_control,
        )
    }

    const fn par(self, right: Self) -> Self {
        if self.role_lane_mask.intersects(&right.role_lane_mask) {
            panic!("parallel lanes must use disjoint (role, lane) pairs");
        }
        let parallel_scope = ScopeId::parallel(0);
        let left_budget = self.scope_budget();
        let right_offset = add_scope_budget(1, left_budget);
        let left_eff = self.into_eff().rebase_scopes(1);
        let right_eff = right.into_eff().rebase_scopes(right_offset);
        Self::from_parts(
            left_eff.extend_list(right_eff).with_scope(parallel_scope),
            self.role_lane_mask.union(right.role_lane_mask),
            false,
            right.tail_is_loop_control,
        )
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) const fn tail_is_loop_control(&self) -> bool {
        self.tail_is_loop_control
    }
}

pub(crate) trait BuildProgramSource {
    const SOURCE: ProgramSourceData;
}

trait VisitProjectionMessages {
    fn visit_projection_messages<V: ProjectionMetadataVisitor>(
        eff_index: u16,
        visitor: &mut V,
    ) -> u16;
}

struct ValidatedProgram<Steps>(PhantomData<Steps>);

impl<Steps> ValidatedProgram<Steps>
where
    Steps: BuildProgramSource,
{
    const SUMMARY: LoweringSummary = {
        let summary = LoweringSummary::scan_const(<Steps as BuildProgramSource>::SOURCE.eff_list());
        summary.validate_projection_program();
        validate_all_roles(&summary);
        summary
    };
}

#[inline(always)]
pub(crate) const fn validated_program_summary<Steps>() -> &'static LoweringSummary
where
    Steps: BuildProgramSource,
{
    &ValidatedProgram::<Steps>::SUMMARY
}

#[cfg(test)]
#[inline(always)]
pub(crate) const fn boundary_source_summary(eff_list: &EffList) -> LoweringSummary {
    LoweringSummary::scan_const(eff_list)
}

impl BuildProgramSource for StepNil {
    const SOURCE: ProgramSourceData = ProgramSourceData::empty();
}

impl VisitProjectionMessages for StepNil {
    fn visit_projection_messages<V: ProjectionMetadataVisitor>(eff_index: u16, _: &mut V) -> u16 {
        eff_index
    }
}

impl<From, To, Msg, const LANE: u8, Tail> BuildProgramSource
    for StepCons<SendStep<From, To, Msg, LANE>, Tail>
where
    From: crate::global::KnownRole + crate::global::RoleMarker,
    To: crate::global::KnownRole + crate::global::RoleMarker,
    Msg: crate::global::MessageSpec
        + crate::global::SendableLabel
        + crate::global::MessageControlSpec,
    Tail: BuildProgramSource,
    StepCons<SendStep<From, To, Msg, LANE>, StepNil>: TailLoopControl,
{
    const SOURCE: ProgramSourceData = ProgramSourceData::from_parts(
        crate::global::const_dsl::const_send_typed::<From, To, Msg, LANE>(),
        RoleLaneMask::empty()
            .with_role(<From as crate::global::KnownRole>::INDEX, LANE)
            .with_role(<To as crate::global::KnownRole>::INDEX, LANE),
        false,
        <StepCons<SendStep<From, To, Msg, LANE>, StepNil> as TailLoopControl>::IS_LOOP_CONTROL,
    )
    .seq(<Tail as BuildProgramSource>::SOURCE);
}

impl<From, To, Msg, const LANE: u8, Tail> VisitProjectionMessages
    for StepCons<SendStep<From, To, Msg, LANE>, Tail>
where
    From: crate::global::KnownRole + crate::global::RoleMarker,
    To: crate::global::KnownRole + crate::global::RoleMarker,
    Msg: crate::global::MessageSpec
        + crate::global::SendableLabel
        + crate::global::MessageControlSpec,
    Tail: VisitProjectionMessages,
{
    fn visit_projection_messages<V: ProjectionMetadataVisitor>(
        eff_index: u16,
        visitor: &mut V,
    ) -> u16 {
        visitor.visit_message(ProjectionMessageSpec {
            eff_index,
            from: <From as crate::global::KnownRole>::INDEX,
            to: <To as crate::global::KnownRole>::INDEX,
            label: <Msg as crate::global::MessageSpec>::LOGICAL_LABEL,
            lane: LANE,
            is_control: <Msg as crate::global::MessageSpec>::CONTROL.is_some(),
            message_type: ProjectionTypeFingerprint::of::<Msg>(),
            payload_type: ProjectionTypeFingerprint::of::<
                <Msg as crate::global::MessageSpec>::Payload,
            >(),
            control_type: ProjectionTypeFingerprint::of::<
                <Msg as crate::global::MessageSpec>::ControlKind,
            >(),
        });
        <Tail as VisitProjectionMessages>::visit_projection_messages(eff_index + 1, visitor)
    }
}

impl<To, Msg, Tail> BuildProgramSource for StepCons<LocalSend<To, Msg>, Tail>
where
    Tail: BuildProgramSource,
{
    const SOURCE: ProgramSourceData = <Tail as BuildProgramSource>::SOURCE;
}

impl<To, Msg, Tail> VisitProjectionMessages for StepCons<LocalSend<To, Msg>, Tail>
where
    Tail: VisitProjectionMessages,
{
    fn visit_projection_messages<V: ProjectionMetadataVisitor>(
        eff_index: u16,
        visitor: &mut V,
    ) -> u16 {
        <Tail as VisitProjectionMessages>::visit_projection_messages(eff_index, visitor)
    }
}

impl<From, Msg, Tail> BuildProgramSource for StepCons<LocalRecv<From, Msg>, Tail>
where
    Tail: BuildProgramSource,
{
    const SOURCE: ProgramSourceData = <Tail as BuildProgramSource>::SOURCE;
}

impl<From, Msg, Tail> VisitProjectionMessages for StepCons<LocalRecv<From, Msg>, Tail>
where
    Tail: VisitProjectionMessages,
{
    fn visit_projection_messages<V: ProjectionMetadataVisitor>(
        eff_index: u16,
        visitor: &mut V,
    ) -> u16 {
        <Tail as VisitProjectionMessages>::visit_projection_messages(eff_index, visitor)
    }
}

impl<Msg, Tail> BuildProgramSource for StepCons<LocalAction<Msg>, Tail>
where
    Tail: BuildProgramSource,
{
    const SOURCE: ProgramSourceData = <Tail as BuildProgramSource>::SOURCE;
}

impl<Msg, Tail> VisitProjectionMessages for StepCons<LocalAction<Msg>, Tail>
where
    Tail: VisitProjectionMessages,
{
    fn visit_projection_messages<V: ProjectionMetadataVisitor>(
        eff_index: u16,
        visitor: &mut V,
    ) -> u16 {
        <Tail as VisitProjectionMessages>::visit_projection_messages(eff_index, visitor)
    }
}

impl<Left, Right> BuildProgramSource for SeqSteps<Left, Right>
where
    Left: BuildProgramSource,
    Right: BuildProgramSource,
{
    const SOURCE: ProgramSourceData =
        <Left as BuildProgramSource>::SOURCE.seq(<Right as BuildProgramSource>::SOURCE);
}

impl<Left, Right> VisitProjectionMessages for SeqSteps<Left, Right>
where
    Left: VisitProjectionMessages,
    Right: VisitProjectionMessages,
{
    fn visit_projection_messages<V: ProjectionMetadataVisitor>(
        eff_index: u16,
        visitor: &mut V,
    ) -> u16 {
        let next = <Left as VisitProjectionMessages>::visit_projection_messages(eff_index, visitor);
        <Right as VisitProjectionMessages>::visit_projection_messages(next, visitor)
    }
}

impl<Left, Right> BuildProgramSource for RouteSteps<Left, Right>
where
    Left: BuildProgramSource + RouteArmHead + RouteArmLoopHead,
    Right: BuildProgramSource + RouteArmHead + RouteArmLoopHead + TailLoopControl,
    <Left as RouteArmHead>::Controller:
        SameRouteControllerRole<<Right as RouteArmHead>::Controller>,
{
    const SOURCE: ProgramSourceData = {
        assert_distinct_route_labels::<<Left as RouteArmHead>::Label, <Right as RouteArmHead>::Label>(
        );
        <Left as BuildProgramSource>::SOURCE.route_with_controller(
            <Right as BuildProgramSource>::SOURCE,
            <<Left as RouteArmHead>::Controller as crate::global::RoleMarker>::INDEX,
            is_binary_loop_route(
                <Left as RouteArmLoopHead>::LOOP_MEANING,
                <Right as RouteArmLoopHead>::LOOP_MEANING,
            ),
        )
    };
}

impl<Left, Right> VisitProjectionMessages for RouteSteps<Left, Right>
where
    Left: VisitProjectionMessages + RouteArmHead + RouteArmLoopHead,
    Right: VisitProjectionMessages + RouteArmHead + RouteArmLoopHead + TailLoopControl,
    <Left as RouteArmHead>::Controller:
        SameRouteControllerRole<<Right as RouteArmHead>::Controller>,
{
    fn visit_projection_messages<V: ProjectionMetadataVisitor>(
        eff_index: u16,
        visitor: &mut V,
    ) -> u16 {
        let next = <Left as VisitProjectionMessages>::visit_projection_messages(eff_index, visitor);
        <Right as VisitProjectionMessages>::visit_projection_messages(next, visitor)
    }
}

impl<Left, Right> BuildProgramSource for ParSteps<Left, Right>
where
    Left: BuildProgramSource + NonEmptyParallelArm,
    Right: BuildProgramSource + NonEmptyParallelArm + TailLoopControl,
{
    const SOURCE: ProgramSourceData =
        { <Left as BuildProgramSource>::SOURCE.par(<Right as BuildProgramSource>::SOURCE) };
}

impl<Left, Right> VisitProjectionMessages for ParSteps<Left, Right>
where
    Left: VisitProjectionMessages + NonEmptyParallelArm,
    Right: VisitProjectionMessages + NonEmptyParallelArm + TailLoopControl,
{
    fn visit_projection_messages<V: ProjectionMetadataVisitor>(
        eff_index: u16,
        visitor: &mut V,
    ) -> u16 {
        let next = <Left as VisitProjectionMessages>::visit_projection_messages(eff_index, visitor);
        <Right as VisitProjectionMessages>::visit_projection_messages(next, visitor)
    }
}

impl<Steps, const POLICY_ID: u16> BuildProgramSource for PolicySteps<Steps, POLICY_ID>
where
    Steps: BuildProgramSource + PolicyEligible,
{
    const SOURCE: ProgramSourceData = <Steps as BuildProgramSource>::SOURCE.with_policy(POLICY_ID);
}

impl<Steps, const POLICY_ID: u16> VisitProjectionMessages for PolicySteps<Steps, POLICY_ID>
where
    Steps: VisitProjectionMessages + PolicyEligible,
{
    fn visit_projection_messages<V: ProjectionMetadataVisitor>(
        eff_index: u16,
        visitor: &mut V,
    ) -> u16 {
        <Steps as VisitProjectionMessages>::visit_projection_messages(eff_index, visitor)
    }
}

/// A typed choreography witness.
///
/// `Program<Steps>` is a zero-sized compile-time proof carrier. It is not a
/// runtime image, not an attached endpoint, and not a transport handle.
///
/// On stable Rust, do not hoist `Program<_>` into `const` or `static` items.
/// Compose programs through a local `let` choreography term and immediately project
/// them through `project(&program)`.
#[derive(Clone, Copy)]
pub struct Program<Steps> {
    steps: PhantomData<Steps>,
}

#[cfg(test)]
impl Program<StepNil> {
    pub(crate) const fn empty() -> Self {
        Self::new()
    }
}

impl<Steps> Program<Steps> {
    #[inline(always)]
    pub(crate) const fn new() -> Self {
        Self { steps: PhantomData }
    }

    pub const fn policy<const POLICY_ID: u16>(self) -> Program<PolicySteps<Steps, POLICY_ID>>
    where
        Steps: PolicyEligible,
    {
        let _ = self;
        Program::new()
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) const fn summary(&self) -> &'static LoweringSummary
    where
        Steps: BuildProgramSource,
    {
        validated_program_summary::<Steps>()
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) const fn tail_is_loop_control(&self) -> bool
    where
        Steps: BuildProgramSource,
    {
        <Steps as BuildProgramSource>::SOURCE.tail_is_loop_control()
    }
}

impl<Universe, Steps> Projectable<Universe> for Program<Steps>
where
    Steps: BuildProgramSource + VisitProjectionMessages,
{
    #[inline(always)]
    fn visit_projection_metadata<V: ProjectionMetadataVisitor>(&self, visitor: &mut V) {
        validated_program_summary::<Steps>().visit_projection_metadata(visitor);
        <Steps as VisitProjectionMessages>::visit_projection_messages(0, visitor);
    }

    #[inline(always)]
    fn project<const ROLE: u8>(&self) -> crate::global::role_program::RoleProgram<ROLE> {
        crate::global::role_program::project(self)
    }
}

pub const fn seq<LeftSteps, RightSteps>(
    left: Program<LeftSteps>,
    right: Program<RightSteps>,
) -> Program<SeqSteps<LeftSteps, RightSteps>> {
    let _ = (left, right);
    Program::new()
}

const fn add_scope_budget(lhs: u16, rhs: u16) -> u16 {
    let sum = lhs as u32 + rhs as u32;
    if sum > ScopeId::ORDINAL_CAPACITY as u32 {
        panic!("structured scope budget exceeded");
    }
    sum as u16
}

pub(crate) const fn route_binary<LeftSteps, RightSteps>(
    left: Program<LeftSteps>,
    right: Program<RightSteps>,
) -> Program<RouteSteps<LeftSteps, RightSteps>>
where
    LeftSteps: RouteArmHead,
    RightSteps: RouteArmHead + TailLoopControl,
    <LeftSteps as RouteArmHead>::Controller:
        SameRouteControllerRole<<RightSteps as RouteArmHead>::Controller>,
{
    assert_distinct_route_labels::<
        <LeftSteps as RouteArmHead>::Label,
        <RightSteps as RouteArmHead>::Label,
    >();
    let _ = (left, right);
    Program::new()
}

pub(crate) const fn par_binary<LeftSteps, RightSteps>(
    left: Program<LeftSteps>,
    right: Program<RightSteps>,
) -> Program<ParSteps<LeftSteps, RightSteps>>
where
    LeftSteps: BuildProgramSource + NonEmptyParallelArm,
    RightSteps: BuildProgramSource + NonEmptyParallelArm + TailLoopControl,
{
    if LeftSteps::SOURCE
        .role_lane_mask()
        .intersects(&RightSteps::SOURCE.role_lane_mask())
    {
        panic!("parallel arms reuse a role on the same lane");
    }
    let _ = (left, right);
    Program::new()
}

const fn is_binary_loop_route(
    left: Option<LoopControlMeaning>,
    right: Option<LoopControlMeaning>,
) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => left.arm() != right.arm(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::Program;
    use crate::g;
    use crate::global::steps::{RouteSteps, SendStep, SeqSteps, StepCons, StepNil};
    use crate::integration::cap::GenericCapToken;
    use crate::integration::cap::advanced::{LoopBreakKind, LoopContinueKind};

    const TEST_LOOP_CONTINUE_LOGICAL: u8 = 0xA1;
    const TEST_LOOP_BREAK_LOGICAL: u8 = 0xA2;

    fn loop_continue_only() -> Program<
        SeqSteps<
            StepCons<
                SendStep<
                    g::Role<0>,
                    g::Role<0>,
                    g::Msg<
                        { TEST_LOOP_CONTINUE_LOGICAL },
                        GenericCapToken<LoopContinueKind>,
                        LoopContinueKind,
                    >,
                >,
                StepNil,
            >,
            StepNil,
        >,
    > {
        g::seq(
            g::send::<
                g::Role<0>,
                g::Role<0>,
                g::Msg<
                    { TEST_LOOP_CONTINUE_LOGICAL },
                    GenericCapToken<LoopContinueKind>,
                    LoopContinueKind,
                >,
                0,
            >(),
            Program::<StepNil>::empty(),
        )
    }

    fn loop_break_only() -> Program<
        StepCons<
            SendStep<
                g::Role<0>,
                g::Role<0>,
                g::Msg<{ TEST_LOOP_BREAK_LOGICAL }, GenericCapToken<LoopBreakKind>, LoopBreakKind>,
            >,
            StepNil,
        >,
    > {
        g::send::<
            g::Role<0>,
            g::Role<0>,
            g::Msg<{ TEST_LOOP_BREAK_LOGICAL }, GenericCapToken<LoopBreakKind>, LoopBreakKind>,
            0,
        >()
    }

    fn loop_decision() -> Program<
        RouteSteps<
            SeqSteps<
                StepCons<
                    SendStep<
                        g::Role<0>,
                        g::Role<0>,
                        g::Msg<
                            { TEST_LOOP_CONTINUE_LOGICAL },
                            GenericCapToken<LoopContinueKind>,
                            LoopContinueKind,
                        >,
                    >,
                    StepNil,
                >,
                StepNil,
            >,
            StepCons<
                SendStep<
                    g::Role<0>,
                    g::Role<0>,
                    g::Msg<
                        { TEST_LOOP_BREAK_LOGICAL },
                        GenericCapToken<LoopBreakKind>,
                        LoopBreakKind,
                    >,
                >,
                StepNil,
            >,
        >,
    > {
        g::route(loop_continue_only(), loop_break_only())
    }

    #[test]
    fn seq_with_empty_suffix_preserves_loop_tail_hint() {
        let composed = g::seq(loop_continue_only(), Program::<StepNil>::empty());
        assert!(
            composed.tail_is_loop_control(),
            "empty seq suffix must preserve loop-control tail hints"
        );
    }

    #[test]
    fn empty_seq_suffix_does_not_change_pending_loop_scope_attachment() {
        let direct = g::seq(loop_continue_only(), loop_decision());
        let nested = g::seq(
            g::seq(loop_continue_only(), Program::<StepNil>::empty()),
            loop_decision(),
        );
        assert!(
            direct.summary().equivalent_summary(nested.summary()),
            "empty seq suffix must not change the validated lowering summary"
        );
    }
}
