//! Typed program representation built from const DSL combinators.
//!
//! `Program<Steps>` pairs the value-level `EffList` with a type-level typelist
//! describing each global send. Projection uses this typelist to recover
//! payload types at compile time.

use core::marker::PhantomData;

use crate::global::const_dsl::{EffList, PolicyMode, ScopeId};
use crate::global::steps::{BuildEffList, PolicyEligible, SeqSteps, StepConcat, StepNil};
use crate::global::{DistinctRouteLabels, NonEmptyParallelArm, RouteArmHead, SameRouteController};
use crate::runtime::consts::{LABEL_LOOP_BREAK, LABEL_LOOP_CONTINUE};

/// Value + type-level representation of a global protocol fragment.
pub struct Program<Steps> {
    eff: EffList,
    scope_budget: u16,
    loop_scope_pending: bool,
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
    const fn wrap_with_hint(eff: EffList, loop_scope_pending: bool) -> Self {
        Self {
            eff,
            scope_budget: eff.scope_budget(),
            loop_scope_pending,
            steps: PhantomData,
        }
    }

    const fn wrap(eff: EffList) -> Self {
        Self::wrap_with_hint(eff, false)
    }

    pub(crate) const fn build() -> Self
    where
        Steps: BuildEffList,
    {
        Self::wrap(Steps::EFF)
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
            let last_atom = eff.last_atom();
            let prev_is_loop_ctrl =
                last_atom.label == LABEL_LOOP_CONTINUE || last_atom.label == LABEL_LOOP_BREAK;
            eff = if prev_is_loop_ctrl {
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
        let (eff, scope_budget) = self.compose_parts(next);
        Program {
            eff,
            scope_budget,
            loop_scope_pending: false,
            steps: PhantomData,
        }
    }
}

pub const fn seq<LeftSteps, RightSteps>(
    left: Program<LeftSteps>,
    right: Program<RightSteps>,
) -> Program<SeqSteps<LeftSteps, RightSteps>> {
    let (eff, scope_budget) = left.compose_parts(right);
    Program {
        eff,
        scope_budget,
        loop_scope_pending: false,
        steps: PhantomData,
    }
}

impl<Steps> Program<Steps>
where
    Steps: PolicyEligible,
{
    pub const fn policy<const POLICY_ID: u16>(self) -> Self {
        Self::wrap(self.eff.with_policy(PolicyMode::dynamic(POLICY_ID)))
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
        + SameRouteController<RightSteps>
        + DistinctRouteLabels<RightSteps>,
    RightSteps: RouteArmHead,
{
    let left_arm = left.into_eff();
    let right_arm = right.into_eff();
    let controller = <<LeftSteps as RouteArmHead>::Controller as crate::global::RoleMarker>::INDEX;

    let left_label = left_arm.first_atom().label;
    let right_label = right_arm.first_atom().label;

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
    let is_loop = is_binary_loop_route(left_label, right_label);
    let eff = left_eff.extend_list(right_eff);
    let eff = if is_loop {
        eff.with_scope_linger(scope, true)
    } else {
        eff
    };
    Program::wrap_with_hint(eff, is_loop)
}

pub(crate) const fn par_binary<LeftSteps, RightSteps>(
    left: Program<LeftSteps>,
    right: Program<RightSteps>,
) -> Program<<LeftSteps as StepConcat<RightSteps>>::Output>
where
    LeftSteps: StepConcat<RightSteps> + NonEmptyParallelArm,
    RightSteps: NonEmptyParallelArm,
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
    Program::wrap(left_eff.extend_list(right_eff).with_scope(parallel_scope))
}

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------

const fn is_binary_loop_route(left_label: u8, right_label: u8) -> bool {
    (left_label == LABEL_LOOP_CONTINUE && right_label == LABEL_LOOP_BREAK)
        || (left_label == LABEL_LOOP_BREAK && right_label == LABEL_LOOP_CONTINUE)
}
