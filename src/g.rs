//! Choreography language used by app authors.
//!
//! `g` is the only app-facing language layer. Build local choreography terms
//! with [`send`], [`seq`], [`route`], and [`par`], then let a protocol crate
//! project and attach them.
//!
//! ```rust,ignore
//! use hibana::g;
//!
//! let request = g::send::<g::Role<0>, g::Role<1>, g::Msg<1, u32>, 0>();
//! let reply = g::send::<g::Role<1>, g::Role<0>, g::Msg<2, u32>, 0>();
//! let program = g::seq(request, reply);
//! ```
//!
//! A [`Msg`] is a typed message descriptor:
//!
//! ```text
//! Msg<LOGICAL_LABEL, Payload, ControlKind = ()>
//! ```
//!
//! Labels identify choreography messages and route branches. They do not encode
//! transport demux or control semantics. Control meaning lives in descriptor
//! metadata derived from the optional `ControlKind`.
//!
//! Dynamic policy is explicit: annotate the choreography point with
//! [`Program::policy`]. Runtime hints or payload contents do not create policy
//! authority by themselves.

use core::marker::PhantomData;

pub use crate::global::MessageSpec;
pub use crate::global::program::Program;
pub use crate::global::{par, route, send, seq};

/// Compile-time role marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Role<const ROLE_INDEX: u8>;

/// Canonical message descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Msg<const LOGICAL_LABEL: u8, Payload, Control = ()>(PhantomData<(Payload, Control)>);

/// Single global send witness.
pub struct Send<From, To, M, const LANE: u8 = 0>(PhantomData<(From, To, M)>);

/// Sequential composition witness.
pub struct Seq<Left, Right>(PhantomData<(Left, Right)>);

/// Binary route witness.
pub struct Route<Left, Right>(PhantomData<(Left, Right)>);

/// Binary parallel composition witness.
pub struct Par<Left, Right>(PhantomData<(Left, Right)>);

/// Dynamic-policy annotation witness.
pub struct Policy<Inner, const POLICY_ID: u16>(PhantomData<Inner>);

use crate::eff::EffIndex;
use crate::global::LoopControlMeaning;
use crate::global::compiled::lowering::{
    CompiledProgramImage, ProgramSourceLookup, validate_all_roles,
};
use crate::global::const_dsl::{EffList, PolicyMode, ScopeId, ScopeKind};
use crate::global::steps::{PolicyEligible, RoleLaneMask};

pub(crate) trait Choreography {
    type Source;
    const SOURCE: Self::Source;
}

#[derive(Clone, Copy)]
pub(crate) struct ProgramSourceData {
    eff: EffList,
    role_lane_mask: RoleLaneMask,
    cycle_scope_pending: bool,
    tail_is_cycle_control: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct RouteHead {
    pub(crate) controller: u8,
    pub(crate) label: u8,
    pub(crate) cycle_meaning: Option<LoopControlMeaning>,
}

impl ProgramSourceData {
    pub(crate) const fn from_parts(
        eff: EffList,
        role_lane_mask: RoleLaneMask,
        cycle_scope_pending: bool,
        tail_is_cycle_control: bool,
    ) -> Self {
        Self {
            eff,
            role_lane_mask,
            cycle_scope_pending,
            tail_is_cycle_control,
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

    pub(crate) const fn route_head(&self) -> RouteHead {
        if self.eff.is_empty() {
            panic!("g::route arms must begin with a controller self-send");
        }
        let node = self.eff.node_at(0);
        if !matches!(node.kind, crate::eff::EffKind::Atom) {
            panic!("g::route arms must begin with a controller self-send");
        }
        let atom = node.atom_data();
        if atom.from != atom.to {
            panic!("g::route arms must begin with a controller self-send");
        }
        RouteHead {
            controller: atom.from,
            label: atom.label,
            cycle_meaning: LoopControlMeaning::from_control_spec(self.eff.control_spec_at(0)),
        }
    }

    pub(crate) const fn seq(self, next: Self) -> Self {
        let next_tail_is_cycle_control = if next.eff.is_empty() {
            self.tail_is_cycle_control
        } else {
            next.tail_is_cycle_control
        };
        let rebased = next.eff.rebase_scopes(self.scope_budget());
        let mut eff = self.eff;
        let scope_budget = self.scope_budget();
        if next.cycle_scope_pending {
            if eff.is_empty() {
                panic!("loop body must contain at least one step");
            }
            let cycle_scope = ScopeId::new(
                ScopeKind::Loop,
                add_scope_budget(scope_budget, next.scope_budget()),
            );
            let scoped_next = rebased.with_scope(cycle_scope);
            eff = if self.tail_is_cycle_control {
                eff.with_scope(cycle_scope).extend_list(scoped_next)
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
            next_tail_is_cycle_control,
        )
    }

    pub(crate) const fn with_policy(self, policy_id: u16) -> Self {
        Self::from_parts(
            self.eff.with_policy(PolicyMode::dynamic(policy_id)),
            self.role_lane_mask,
            self.cycle_scope_pending,
            self.tail_is_cycle_control,
        )
    }

    pub(crate) const fn route_with_controller(
        self,
        right: Self,
        controller: u8,
        is_cycle: bool,
    ) -> Self {
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
        let eff = if is_cycle {
            eff.with_scope_linger(scope, true)
        } else {
            eff
        };
        let cycle_scope_pending = eff.scope_has_linger(scope);
        Self::from_parts(
            eff,
            self.role_lane_mask.union(right.role_lane_mask),
            cycle_scope_pending,
            right.tail_is_cycle_control,
        )
    }

    pub(crate) const fn par(self, right: Self) -> Self {
        if self.eff.is_empty() || right.eff.is_empty() {
            panic!("g::par(left, right) arms must be non-empty protocol fragments");
        }
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
            right.tail_is_cycle_control,
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

const fn is_binary_cycle_route(
    left: Option<LoopControlMeaning>,
    right: Option<LoopControlMeaning>,
) -> bool {
    match (left, right) {
        (Some(LoopControlMeaning::Continue), Some(LoopControlMeaning::Break)) => true,
        (Some(_), Some(_)) => panic!("loop routes must order arms as continue then break"),
        (Some(_), None) | (None, Some(_)) => {
            panic!("loop routes must pair continue and break control arms")
        }
        _ => false,
    }
}

struct ProjectedChoreography<Steps>(PhantomData<Steps>);

impl<Steps> ProjectedChoreography<Steps>
where
    Steps: Choreography<Source = ProgramSourceData>,
{
    fn source_policy_at(offset: usize) -> Option<PolicyMode> {
        <Steps as Choreography>::SOURCE
            .eff_list()
            .policy_with_scope(offset)
            .map(|(policy, _scope)| policy)
    }

    fn source_control_desc_at(offset: usize) -> Option<crate::global::ControlDesc> {
        let spec = <Steps as Choreography>::SOURCE
            .eff_list()
            .control_spec_at(offset)?;
        Some(crate::global::ControlDesc::from_static(spec).with_sites(
            EffIndex::from_dense_ordinal(offset),
            crate::global::ControlDesc::STATIC_POLICY_SITE,
        ))
    }

    const PROGRAM_IMAGE: CompiledProgramImage = {
        let source = <Steps as Choreography>::SOURCE.eff_list();
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
pub(crate) const fn projected_choreography_image<Steps>() -> &'static CompiledProgramImage
where
    Steps: Choreography<Source = ProgramSourceData>,
{
    &ProjectedChoreography::<Steps>::PROGRAM_IMAGE
}

impl<From, To, M, const LANE: u8> Choreography for Send<From, To, M, LANE>
where
    From: crate::global::KnownRole + crate::global::RoleMarker,
    To: crate::global::KnownRole + crate::global::RoleMarker,
    M: crate::global::MessageSpec,
{
    type Source = ProgramSourceData;
    const SOURCE: Self::Source = {
        let control = <M as crate::global::MessageRuntime>::CONTROL;
        ProgramSourceData::from_parts(
            crate::global::const_dsl::const_send_typed::<From, To, M, LANE>(),
            RoleLaneMask::empty()
                .with_role(<From as crate::global::KnownRole>::INDEX, LANE)
                .with_role(<To as crate::global::KnownRole>::INDEX, LANE),
            false,
            LoopControlMeaning::from_control_spec(control).is_some(),
        )
    };
}

impl<Left, Right> Choreography for Seq<Left, Right>
where
    Left: Choreography<Source = ProgramSourceData>,
    Right: Choreography<Source = ProgramSourceData>,
{
    type Source = ProgramSourceData;
    const SOURCE: Self::Source =
        <Left as Choreography>::SOURCE.seq(<Right as Choreography>::SOURCE);
}

impl<Left, Right> Choreography for Route<Left, Right>
where
    Left: Choreography<Source = ProgramSourceData>,
    Right: Choreography<Source = ProgramSourceData>,
{
    type Source = ProgramSourceData;
    const SOURCE: Self::Source = {
        let left = <Left as Choreography>::SOURCE;
        let right = <Right as Choreography>::SOURCE;
        let left_head = left.route_head();
        let right_head = right.route_head();
        if left_head.label == right_head.label {
            panic!("route arms reuse the same label");
        }
        if left_head.controller != right_head.controller {
            panic!("route arms use different controller self-sends");
        }
        left.route_with_controller(
            right,
            left_head.controller,
            is_binary_cycle_route(left_head.cycle_meaning, right_head.cycle_meaning),
        )
    };
}

impl<Left, Right> Choreography for Par<Left, Right>
where
    Left: Choreography<Source = ProgramSourceData>,
    Right: Choreography<Source = ProgramSourceData>,
{
    type Source = ProgramSourceData;
    const SOURCE: Self::Source =
        { <Left as Choreography>::SOURCE.par(<Right as Choreography>::SOURCE) };
}

impl<Steps, const POLICY_ID: u16> Choreography for Policy<Steps, POLICY_ID>
where
    Steps: Choreography<Source = ProgramSourceData> + PolicyEligible,
{
    type Source = ProgramSourceData;
    const SOURCE: Self::Source = {
        if POLICY_ID == crate::global::ControlDesc::STATIC_POLICY_SITE {
            panic!("dynamic policy id u16::MAX is reserved for static policy");
        }
        <Steps as Choreography>::SOURCE.with_policy(POLICY_ID)
    };
}
