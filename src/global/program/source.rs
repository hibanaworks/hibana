use core::marker::PhantomData;

use crate::eff::EffIndex;
use crate::global::compiled::lowering::{
    CompiledProgramImage, ProgramSourceLookup, validate_all_roles,
};
use crate::global::const_dsl::{EffList, PolicyMode, ScopeId};
use crate::global::steps::{PolicyEligible, RoleLaneMask};
use crate::global::{
    ControlDesc, NonEmptyParallelArm, RouteArmHead, RouteArmLoopHead, SameRouteControllerRole,
    TailLoopControl, assert_distinct_route_labels,
};

use super::{add_scope_budget, is_binary_loop_route};

#[derive(Clone, Copy)]
pub struct ProgramSourceData {
    eff: EffList,
    role_lane_mask: RoleLaneMask,
    loop_scope_pending: bool,
    tail_is_loop_control: bool,
}

impl ProgramSourceData {
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
        if policy_id == ControlDesc::STATIC_POLICY_SITE {
            panic!("dynamic policy id u16::MAX is reserved for static policy");
        }
        Self::from_parts(
            self.eff.with_policy(PolicyMode::dynamic(policy_id)),
            self.role_lane_mask,
            self.loop_scope_pending,
            self.tail_is_loop_control,
        )
    }

    const fn route_with_controller(self, right: Self, controller: u8, is_loop: bool) -> Self {
        let scope = ScopeId::route(0);
        self.eff.assert_route_arm_dynamic_policy_head();
        right.eff.assert_route_arm_dynamic_policy_head();
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
}

pub trait BuildProgramSource {
    const SOURCE: ProgramSourceData;
}

struct ValidatedProgram<Steps>(PhantomData<Steps>);

impl<Steps> ValidatedProgram<Steps>
where
    Steps: BuildProgramSource,
{
    fn source_policy_at(offset: usize) -> Option<PolicyMode> {
        <Steps as BuildProgramSource>::SOURCE
            .eff_list()
            .policy_with_scope(offset)
            .map(|(policy, _scope)| policy)
    }

    fn source_control_desc_at(offset: usize) -> Option<ControlDesc> {
        let spec = <Steps as BuildProgramSource>::SOURCE
            .eff_list()
            .control_spec_at(offset)?;
        Some(ControlDesc::from_static(spec).with_sites(
            EffIndex::from_dense_ordinal(offset),
            ControlDesc::STATIC_POLICY_SITE,
        ))
    }

    const PROGRAM_IMAGE: CompiledProgramImage = {
        let source = <Steps as BuildProgramSource>::SOURCE.eff_list();
        let image = CompiledProgramImage::scan_const_with_lookup(
            source,
            ProgramSourceLookup::new(Self::source_policy_at, Self::source_control_desc_at),
        );
        image.validate_projection_program();
        validate_all_roles(&image, source);
        image
    };
}

#[inline(always)]
pub(crate) const fn validated_program_image<Steps>() -> &'static CompiledProgramImage
where
    Steps: BuildProgramSource,
{
    &ValidatedProgram::<Steps>::PROGRAM_IMAGE
}

#[cfg(test)]
#[inline(always)]
pub(crate) const fn boundary_source_program_image(eff_list: &EffList) -> CompiledProgramImage {
    CompiledProgramImage::scan_const(eff_list)
}

impl<From, To, Msg, const LANE: u8> BuildProgramSource for crate::g::Send<From, To, Msg, LANE>
where
    From: crate::global::KnownRole + crate::global::RoleMarker,
    To: crate::global::KnownRole + crate::global::RoleMarker,
    Msg: crate::global::MessageSpec
        + crate::global::SendableLabel
        + crate::global::MessageControlSpec,
    crate::g::Send<From, To, Msg, LANE>: TailLoopControl,
{
    const SOURCE: ProgramSourceData = ProgramSourceData::from_parts(
        crate::global::const_dsl::const_send_typed::<From, To, Msg, LANE>(),
        RoleLaneMask::empty()
            .with_role(<From as crate::global::KnownRole>::INDEX, LANE)
            .with_role(<To as crate::global::KnownRole>::INDEX, LANE),
        false,
        <crate::g::Send<From, To, Msg, LANE> as TailLoopControl>::IS_LOOP_CONTROL,
    );
}

impl<Left, Right> BuildProgramSource for crate::g::Seq<Left, Right>
where
    Left: BuildProgramSource,
    Right: BuildProgramSource,
{
    const SOURCE: ProgramSourceData =
        <Left as BuildProgramSource>::SOURCE.seq(<Right as BuildProgramSource>::SOURCE);
}

impl<Left, Right> BuildProgramSource for crate::g::Route<Left, Right>
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

impl<Left, Right> BuildProgramSource for crate::g::Par<Left, Right>
where
    Left: BuildProgramSource + NonEmptyParallelArm,
    Right: BuildProgramSource + NonEmptyParallelArm + TailLoopControl,
{
    const SOURCE: ProgramSourceData =
        { <Left as BuildProgramSource>::SOURCE.par(<Right as BuildProgramSource>::SOURCE) };
}

impl<Steps, const POLICY_ID: u16> BuildProgramSource for crate::g::Policy<Steps, POLICY_ID>
where
    Steps: BuildProgramSource + PolicyEligible,
{
    const SOURCE: ProgramSourceData = <Steps as BuildProgramSource>::SOURCE.with_policy(POLICY_ID);
}
