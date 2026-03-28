//! Typed program representation built from const DSL combinators.
//!
//! `Program<Steps>` pairs the value-level `EffList` with a type-level typelist
//! describing each global send. Projection uses this typelist to recover
//! payload types at compile time.

use core::marker::PhantomData;

use crate::global::const_dsl::{EffList, PolicyMode, ScopeId};
use crate::global::steps::{BuildEffList, PolicyEligible, SeqSteps, StepConcat, StepNil};
use crate::global::{
    DistinctRouteLabels, LoopControlMeaning, NonEmptyParallelArm, RouteArmHead, RouteArmLoopHead,
    SameRouteController, TailLoopControl,
};

/// Value + type-level representation of a global protocol fragment.
pub struct Program<Steps> {
    eff: EffList,
    scope_budget: u16,
    loop_scope_pending: bool,
    tail_is_loop_control: bool,
    steps: PhantomData<Steps>,
}

impl<Steps> Clone for Program<Steps> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<Steps> Copy for Program<Steps> {}

impl Program<StepNil> {
    pub(crate) const fn empty() -> Self {
        Self::build()
    }
}

impl<Steps> Program<Steps> {
    const fn wrap_with_hint(
        eff: EffList,
        loop_scope_pending: bool,
        tail_is_loop_control: bool,
    ) -> Self {
        Self {
            eff,
            scope_budget: eff.scope_budget(),
            loop_scope_pending,
            tail_is_loop_control,
            steps: PhantomData,
        }
    }

    pub(crate) const fn build() -> Self
    where
        Steps: BuildEffList + TailLoopControl,
    {
        Self::wrap_with_hint(
            Steps::EFF,
            false,
            <Steps as TailLoopControl>::IS_LOOP_CONTROL,
        )
    }

    pub(crate) const fn eff_list(&self) -> &EffList {
        &self.eff
    }

    pub(crate) const fn into_eff(self) -> EffList {
        self.eff
    }

    pub(crate) const fn scope_budget(&self) -> u16 {
        self.scope_budget
    }

    const fn compose_parts<NextSteps>(self, next: Program<NextSteps>) -> (EffList, u16) {
        let rebased = next.eff.rebase_scopes(self.scope_budget);
        let mut eff = self.eff;
        let mut scope_budget = self.scope_budget;
        if next.loop_scope_pending {
            if eff.is_empty() {
                panic!("loop body must contain at least one step");
            }
            let loop_scope = ScopeId::loop_scope(add_scope_budget(scope_budget, next.scope_budget));
            let scoped_next = rebased.with_scope(loop_scope);
            eff = if self.tail_is_loop_control {
                eff.with_scope(loop_scope).extend_list(scoped_next)
            } else {
                eff.extend_list(scoped_next)
            };
            scope_budget = add_scope_budget(scope_budget, add_scope_budget(next.scope_budget, 1));
        } else {
            eff = eff.extend_list(rebased);
            scope_budget = add_scope_budget(scope_budget, next.scope_budget);
        }
        (eff, scope_budget)
    }

    pub(crate) const fn then<NextSteps>(
        self,
        next: Program<NextSteps>,
    ) -> Program<<Steps as StepConcat<NextSteps>>::Output>
    where
        Steps: StepConcat<NextSteps>,
    {
        let next_tail_is_loop_control = if next.eff.is_empty() {
            self.tail_is_loop_control
        } else {
            next.tail_is_loop_control
        };
        let (eff, scope_budget) = self.compose_parts(next);
        Program {
            eff,
            scope_budget,
            loop_scope_pending: false,
            tail_is_loop_control: next_tail_is_loop_control,
            steps: PhantomData,
        }
    }
}

pub const fn seq<LeftSteps, RightSteps>(
    left: Program<LeftSteps>,
    right: Program<RightSteps>,
) -> Program<SeqSteps<LeftSteps, RightSteps>> {
    let right_tail_is_loop_control = if right.eff.is_empty() {
        left.tail_is_loop_control
    } else {
        right.tail_is_loop_control
    };
    let (eff, scope_budget) = left.compose_parts(right);
    Program {
        eff,
        scope_budget,
        loop_scope_pending: false,
        tail_is_loop_control: right_tail_is_loop_control,
        steps: PhantomData,
    }
}

impl<Steps> Program<Steps>
where
    Steps: PolicyEligible,
{
    pub const fn policy<const POLICY_ID: u16>(self) -> Self {
        Self::wrap_with_hint(
            self.eff.with_policy(PolicyMode::dynamic(POLICY_ID)),
            self.loop_scope_pending,
            self.tail_is_loop_control,
        )
    }
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
) -> Program<<LeftSteps as StepConcat<RightSteps>>::Output>
where
    LeftSteps: StepConcat<RightSteps>
        + RouteArmHead
        + RouteArmLoopHead
        + SameRouteController<RightSteps>
        + DistinctRouteLabels<RightSteps>,
    RightSteps: RouteArmHead + RouteArmLoopHead + TailLoopControl,
{
    let left_arm = left.into_eff();
    let right_arm = right.into_eff();
    let controller = <<LeftSteps as RouteArmHead>::Controller as crate::global::RoleMarker>::INDEX;

    let scope = ScopeId::route(0);
    let left_budget = left.scope_budget();
    let right_offset = add_scope_budget(1, left_budget);
    let left_eff = left_arm
        .rebase_scopes(1)
        .with_scope_controller(scope, controller);
    let right_eff = right_arm
        .rebase_scopes(right_offset)
        .with_scope(scope)
        .with_scope_controller_role(scope, controller);
    let is_loop = is_binary_loop_route(
        <LeftSteps as RouteArmLoopHead>::LOOP_MEANING,
        <RightSteps as RouteArmLoopHead>::LOOP_MEANING,
    );
    let eff = left_eff.extend_list(right_eff);
    let eff = if is_loop {
        eff.with_scope_linger(scope, true)
    } else {
        eff
    };
    let loop_scope_pending = eff.scope_has_linger(scope);
    Program::wrap_with_hint(
        eff,
        loop_scope_pending,
        <RightSteps as TailLoopControl>::IS_LOOP_CONTROL,
    )
}

pub(crate) const fn par_binary<LeftSteps, RightSteps>(
    left: Program<LeftSteps>,
    right: Program<RightSteps>,
) -> Program<<LeftSteps as StepConcat<RightSteps>>::Output>
where
    LeftSteps: StepConcat<RightSteps> + NonEmptyParallelArm,
    RightSteps: NonEmptyParallelArm + TailLoopControl,
{
    let left_set = <LeftSteps as NonEmptyParallelArm>::ROLE_LANE_SET;
    let right_set = <RightSteps as NonEmptyParallelArm>::ROLE_LANE_SET;
    if left_set.intersects(&right_set) {
        panic!("parallel lanes must use disjoint (role, lane) pairs");
    }

    let parallel_scope = ScopeId::parallel(0);
    let left_budget = left.scope_budget();
    let right_offset = add_scope_budget(1, left_budget);
    let left_eff = left.into_eff().rebase_scopes(1);
    let right_eff = right.into_eff().rebase_scopes(right_offset);
    Program::wrap_with_hint(
        left_eff.extend_list(right_eff).with_scope(parallel_scope),
        false,
        <RightSteps as TailLoopControl>::IS_LOOP_CONTROL,
    )
}

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------

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
    use crate::g::advanced::CanonicalControl;
    use crate::g::advanced::steps::{
        LoopBreakSteps, LoopContinueSteps, LoopDecisionSteps, StepNil,
    };
    use crate::global::compiled::LoweringSummary;
    use crate::runtime::consts::{LABEL_LOOP_BREAK, LABEL_LOOP_CONTINUE};
    use crate::substrate::cap::GenericCapToken;
    use crate::substrate::cap::advanced::{LoopBreakKind, LoopContinueKind};

    const LOOP_CONTINUE_ONLY: Program<
        LoopContinueSteps<
            g::Role<0>,
            g::Msg<
                { LABEL_LOOP_CONTINUE },
                GenericCapToken<LoopContinueKind>,
                CanonicalControl<LoopContinueKind>,
            >,
            StepNil,
        >,
    > = g::advanced::compose::seq(
        g::send::<
            g::Role<0>,
            g::Role<0>,
            g::Msg<
                { LABEL_LOOP_CONTINUE },
                GenericCapToken<LoopContinueKind>,
                CanonicalControl<LoopContinueKind>,
            >,
            0,
        >(),
        StepNil::PROGRAM,
    );

    const LOOP_BREAK_ONLY: Program<
        LoopBreakSteps<
            g::Role<0>,
            g::Msg<
                { LABEL_LOOP_BREAK },
                GenericCapToken<LoopBreakKind>,
                CanonicalControl<LoopBreakKind>,
            >,
            StepNil,
        >,
    > = g::send::<
        g::Role<0>,
        g::Role<0>,
        g::Msg<
            { LABEL_LOOP_BREAK },
            GenericCapToken<LoopBreakKind>,
            CanonicalControl<LoopBreakKind>,
        >,
        0,
    >();

    const LOOP_DECISION: Program<
        LoopDecisionSteps<
            g::Role<0>,
            g::Msg<
                { LABEL_LOOP_CONTINUE },
                GenericCapToken<LoopContinueKind>,
                CanonicalControl<LoopContinueKind>,
            >,
            g::Msg<
                { LABEL_LOOP_BREAK },
                GenericCapToken<LoopBreakKind>,
                CanonicalControl<LoopBreakKind>,
            >,
            StepNil,
            StepNil,
        >,
    > = g::route(LOOP_CONTINUE_ONLY, LOOP_BREAK_ONLY);

    #[test]
    fn seq_with_empty_suffix_preserves_loop_tail_hint() {
        let composed = g::advanced::compose::seq(LOOP_CONTINUE_ONLY, StepNil::PROGRAM);
        assert!(
            composed.tail_is_loop_control,
            "empty seq suffix must preserve loop-control tail hints"
        );
    }

    #[test]
    fn empty_seq_suffix_does_not_change_pending_loop_scope_attachment() {
        let direct = g::seq(LOOP_CONTINUE_ONLY, LOOP_DECISION);
        let nested = g::seq(
            g::advanced::compose::seq(LOOP_CONTINUE_ONLY, StepNil::PROGRAM),
            LOOP_DECISION,
        );
        assert!(
            LoweringSummary::scan_const(direct.eff_list()).equivalent_eff_list(nested.eff_list()),
            "empty seq suffix must not change the loop-scoped effect list"
        );
    }
}
