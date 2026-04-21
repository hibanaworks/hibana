//! Typed program representation built from const DSL combinators.
//!
//! `Program<Steps>` is the public choreography owner consumed by projection and
//! attach paths. The raw `EffList` source and cheap composition hints stay
//! crate-private behind type-level source builders.

use core::marker::PhantomData;

use crate::global::compiled::lowering::{LoweringSummary, ProgramStamp, validate_all_roles};
use crate::global::const_dsl::{EffList, PolicyMode, ScopeId};
use crate::global::steps::{
    LocalAction, LocalRecv, LocalSend, ParSteps, PolicyEligible, PolicySteps, RoleEq, RouteSteps,
    SendStep, SeqSteps, StepCons, StepNil,
};
use crate::global::{
    DistinctRouteLabels, LoopControlMeaning, NonEmptyParallelArm, RouteArmHead, RouteArmLoopHead,
    SameRouteController, TailLoopControl,
};

#[derive(Clone, Copy)]
pub(crate) struct ProgramSourceData {
    eff: EffList,
    loop_scope_pending: bool,
    tail_is_loop_control: bool,
}

impl ProgramSourceData {
    pub(crate) const fn empty() -> Self {
        Self::from_eff(EffList::new(), false, false)
    }

    const fn from_eff(eff: EffList, loop_scope_pending: bool, tail_is_loop_control: bool) -> Self {
        Self {
            eff,
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

    const fn seq(self, next: Self) -> Self {
        let next_tail_is_loop_control = if next.eff.is_empty() {
            self.tail_is_loop_control
        } else {
            next.tail_is_loop_control
        };
        let rebased = next.eff.rebase_scopes(self.scope_budget());
        let mut eff = self.eff;
        let mut scope_budget = self.scope_budget();
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
            scope_budget = add_scope_budget(scope_budget, add_scope_budget(next.scope_budget(), 1));
        } else {
            eff = eff.extend_list(rebased);
            scope_budget = add_scope_budget(scope_budget, next.scope_budget());
        }
        let _ = scope_budget;
        Self::from_eff(eff, false, next_tail_is_loop_control)
    }

    const fn with_policy(self, policy_id: u16) -> Self {
        Self::from_eff(
            self.eff.with_policy(PolicyMode::dynamic(policy_id)),
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
        Self::from_eff(eff, loop_scope_pending, right.tail_is_loop_control)
    }

    const fn par(self, right: Self) -> Self {
        let parallel_scope = ScopeId::parallel(0);
        let left_budget = self.scope_budget();
        let right_offset = add_scope_budget(1, left_budget);
        let left_eff = self.into_eff().rebase_scopes(1);
        let right_eff = right.into_eff().rebase_scopes(right_offset);
        Self::from_eff(
            left_eff.extend_list(right_eff).with_scope(parallel_scope),
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

    const STAMP: ProgramStamp = Self::SUMMARY.stamp();
}

#[inline(always)]
pub(crate) const fn validated_program_stamp<Steps>() -> ProgramStamp
where
    Steps: BuildProgramSource,
{
    ValidatedProgram::<Steps>::STAMP
}

#[inline(always)]
pub(crate) const fn validated_program_summary<Steps>() -> &'static LoweringSummary
where
    Steps: BuildProgramSource,
{
    &ValidatedProgram::<Steps>::SUMMARY
}

impl BuildProgramSource for StepNil {
    const SOURCE: ProgramSourceData = ProgramSourceData::empty();
}

impl<From, To, Msg, const LANE: u8, Tail> BuildProgramSource
    for StepCons<SendStep<From, To, Msg, LANE>, Tail>
where
    From: crate::global::KnownRole + crate::global::RoleMarker + RoleEq<To>,
    To: crate::global::KnownRole + crate::global::RoleMarker,
    Msg: crate::global::MessageSpec
        + crate::global::SendableLabel
        + crate::global::MessageControlSpec,
    Tail: BuildProgramSource,
    StepCons<SendStep<From, To, Msg, LANE>, StepNil>: TailLoopControl,
{
    const SOURCE: ProgramSourceData = ProgramSourceData::from_eff(
        crate::global::const_dsl::const_send_typed::<From, To, Msg, LANE>(),
        false,
        <StepCons<SendStep<From, To, Msg, LANE>, StepNil> as TailLoopControl>::IS_LOOP_CONTROL,
    )
    .seq(<Tail as BuildProgramSource>::SOURCE);
}

impl<To, Msg, Tail> BuildProgramSource for StepCons<LocalSend<To, Msg>, Tail>
where
    Tail: BuildProgramSource,
{
    const SOURCE: ProgramSourceData = <Tail as BuildProgramSource>::SOURCE;
}

impl<From, Msg, Tail> BuildProgramSource for StepCons<LocalRecv<From, Msg>, Tail>
where
    Tail: BuildProgramSource,
{
    const SOURCE: ProgramSourceData = <Tail as BuildProgramSource>::SOURCE;
}

impl<Msg, Tail> BuildProgramSource for StepCons<LocalAction<Msg>, Tail>
where
    Tail: BuildProgramSource,
{
    const SOURCE: ProgramSourceData = <Tail as BuildProgramSource>::SOURCE;
}

impl<Left, Right> BuildProgramSource for SeqSteps<Left, Right>
where
    Left: BuildProgramSource,
    Right: BuildProgramSource,
{
    const SOURCE: ProgramSourceData =
        <Left as BuildProgramSource>::SOURCE.seq(<Right as BuildProgramSource>::SOURCE);
}

impl<Left, Right> BuildProgramSource for RouteSteps<Left, Right>
where
    Left: BuildProgramSource
        + RouteArmHead
        + RouteArmLoopHead
        + SameRouteController<Right>
        + DistinctRouteLabels<Right>,
    Right: BuildProgramSource + RouteArmHead + RouteArmLoopHead + TailLoopControl,
{
    const SOURCE: ProgramSourceData = <Left as BuildProgramSource>::SOURCE.route_with_controller(
        <Right as BuildProgramSource>::SOURCE,
        <<Left as RouteArmHead>::Controller as crate::global::RoleMarker>::INDEX,
        is_binary_loop_route(
            <Left as RouteArmLoopHead>::LOOP_MEANING,
            <Right as RouteArmLoopHead>::LOOP_MEANING,
        ),
    );
}

impl<Left, Right> BuildProgramSource for ParSteps<Left, Right>
where
    Left: BuildProgramSource + NonEmptyParallelArm,
    Right: BuildProgramSource + NonEmptyParallelArm + TailLoopControl,
{
    const SOURCE: ProgramSourceData = {
        let left_set = <Left as NonEmptyParallelArm>::ROLE_LANE_SET;
        let right_set = <Right as NonEmptyParallelArm>::ROLE_LANE_SET;
        if left_set.intersects(&right_set) {
            panic!("parallel lanes must use disjoint (role, lane) pairs");
        }
        <Left as BuildProgramSource>::SOURCE.par(<Right as BuildProgramSource>::SOURCE)
    };
}

impl<Steps, const POLICY_ID: u16> BuildProgramSource for PolicySteps<Steps, POLICY_ID>
where
    Steps: BuildProgramSource + PolicyEligible,
{
    const SOURCE: ProgramSourceData = <Steps as BuildProgramSource>::SOURCE.with_policy(POLICY_ID);
}

/// A typed choreography witness.
///
/// `Program<Steps>` is a zero-sized compile-time proof carrier. It is not a
/// runtime image, not an attached endpoint, and not a transport handle.
///
/// On stable Rust, do not write `const APP: Program<_>`.
/// Compose programs through local `let` inference and immediately project
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
    const fn new() -> Self {
        Self { steps: PhantomData }
    }

    pub(crate) const fn build() -> Self
    where
        Steps: BuildProgramSource,
    {
        let _ = <Steps as BuildProgramSource>::SOURCE.scope_budget();
        Self::new()
    }

    pub const fn policy<const POLICY_ID: u16>(self) -> Program<PolicySteps<Steps, POLICY_ID>>
    where
        Steps: PolicyEligible,
    {
        let _ = self;
        Program::new()
    }

    #[inline(always)]
    pub(crate) const fn stamp(&self) -> ProgramStamp
    where
        Steps: BuildProgramSource,
    {
        validated_program_stamp::<Steps>()
    }

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
    LeftSteps: RouteArmHead + SameRouteController<RightSteps> + DistinctRouteLabels<RightSteps>,
    RightSteps: RouteArmHead + TailLoopControl,
{
    let _ = (left, right);
    Program::new()
}

pub(crate) const fn par_binary<LeftSteps, RightSteps>(
    left: Program<LeftSteps>,
    right: Program<RightSteps>,
) -> Program<ParSteps<LeftSteps, RightSteps>>
where
    LeftSteps: NonEmptyParallelArm,
    RightSteps: NonEmptyParallelArm + TailLoopControl,
{
    if LeftSteps::ROLE_LANE_SET.intersects(&RightSteps::ROLE_LANE_SET) {
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
    use crate::global::steps::{LoopBreakSteps, LoopContinueSteps, LoopDecisionSteps, StepNil};
    use crate::runtime::consts::{LABEL_LOOP_BREAK, LABEL_LOOP_CONTINUE};
    use crate::substrate::cap::GenericCapToken;
    use crate::substrate::cap::advanced::{LoopBreakKind, LoopContinueKind};

    fn loop_continue_only() -> Program<
        LoopContinueSteps<
            g::Role<0>,
            g::Msg<{ LABEL_LOOP_CONTINUE }, GenericCapToken<LoopContinueKind>, LoopContinueKind>,
            StepNil,
        >,
    > {
        g::seq(
            g::send::<
                g::Role<0>,
                g::Role<0>,
                g::Msg<
                    { LABEL_LOOP_CONTINUE },
                    GenericCapToken<LoopContinueKind>,
                    LoopContinueKind,
                >,
                0,
            >(),
            Program::<StepNil>::empty(),
        )
    }

    fn loop_break_only() -> Program<
        LoopBreakSteps<
            g::Role<0>,
            g::Msg<{ LABEL_LOOP_BREAK }, GenericCapToken<LoopBreakKind>, LoopBreakKind>,
            StepNil,
        >,
    > {
        g::send::<
            g::Role<0>,
            g::Role<0>,
            g::Msg<{ LABEL_LOOP_BREAK }, GenericCapToken<LoopBreakKind>, LoopBreakKind>,
            0,
        >()
    }

    fn loop_decision() -> Program<
        LoopDecisionSteps<
            g::Role<0>,
            g::Msg<{ LABEL_LOOP_CONTINUE }, GenericCapToken<LoopContinueKind>, LoopContinueKind>,
            g::Msg<{ LABEL_LOOP_BREAK }, GenericCapToken<LoopBreakKind>, LoopBreakKind>,
            StepNil,
            StepNil,
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
