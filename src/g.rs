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

mod terms;

use core::marker::PhantomData;

pub use crate::global::MessageSpec;
pub use crate::global::{par, route, send, seq};

/// Compile-time role marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Role<const ROLE_INDEX: u8>;

/// Canonical message descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Msg<const LOGICAL_LABEL: u8, Payload, Control = ()>(PhantomData<(Payload, Control)>);

#[derive(Clone, Copy)]
#[repr(u8)]
pub(crate) enum ProgramSourceError {
    RouteArmHead,
    RouteDuplicateLabel,
    RouteControllerMismatch,
    LoopRouteArmOrder,
    LoopRouteArmPair,
    LoopBodyEmpty,
    ParallelEmpty,
    ParallelConflict,
    PolicyIdReserved,
    PolicyNotHead,
    PolicyRequiresControlHead,
    PolicyUnsupportedControlHead,
    ProjectionRoutePolicyMismatch,
    ProjectionRoutePolicyMissing,
    ProjectionRouteUnprojectable,
}

impl ProgramSourceError {
    pub(crate) const fn from_policy_head_status(status: u8) -> Option<Self> {
        match status {
            0 => None,
            1 => Some(Self::PolicyNotHead),
            2 => Some(Self::PolicyRequiresControlHead),
            _ => Some(Self::PolicyUnsupportedControlHead),
        }
    }

    #[cfg(all(test, hibana_repo_tests))]
    pub(crate) const fn panic_repo_test(self) -> ! {
        panic_program_source_error(self)
    }
}

const fn panic_program_source_error(error: ProgramSourceError) -> ! {
    match error as u8 {
        0 => panic!("g::route arms must begin with a controller self-send"),
        1 => panic!("route arms reuse the same label"),
        2 => panic!("route arms use different controller self-sends"),
        3 => panic!("loop routes must order arms as continue then break"),
        4 => panic!("loop routes must pair continue and break control arms"),
        5 => panic!("loop body must contain at least one step"),
        6 => {
            panic!("g::par(left, right) arms must be non-empty protocol fragments")
        }
        7 => {
            panic!("parallel lanes must use disjoint (role, lane) pairs")
        }
        8 => {
            panic!("dynamic policy id u16::MAX is reserved for static policy")
        }
        9 => {
            panic!(
                "Program::policy must annotate the controller self-send that opens each route/loop arm"
            )
        }
        10 => {
            panic!("Program::policy requires a route/loop controller self-send head")
        }
        11 => {
            panic!("Program::policy supports only route/loop controller self-send heads")
        }
        12 => panic!("route policy mismatch"),
        13 => panic!("route policy missing"),
        _ => panic!(concat!(
            "Route unprojectable for this role: arms not mergeable, ",
            "wire dispatch non-deterministic, ",
            "and no dynamic policy annotation provided",
        )),
    }
}

/// A typed choreography term.
///
/// `Program<Steps>` is a zero-sized compile-time choreography value. Projection
/// validates it and returns the proof-carrying `RoleProgram`; the unprojected
/// term is not a runtime image, not an attached endpoint, and not a transport
/// handle.
///
/// On stable Rust, do not hoist `Program<_>` into `const` or `static` items.
/// Compose programs through a local `let` choreography term and immediately
/// project them through `project(&program)`.
#[derive(Clone, Copy)]
pub struct Program<Steps> {
    steps: PhantomData<Steps>,
}

impl<Steps> Program<Steps> {
    #[inline(always)]
    pub(crate) const fn new() -> Self {
        Self { steps: PhantomData }
    }
}

struct ProgramProjection<Steps>(PhantomData<Steps>);

impl<Steps> ProgramProjection<Steps>
where
    Steps: ProgramTerm<Source = ProgramSourceData>,
{
    fn source_policy_at(offset: usize) -> Option<crate::global::const_dsl::PolicyMode> {
        <Steps as ProgramTerm>::PROGRAM_SOURCE
            .eff_list()
            .policy_with_scope(offset)
            .map(|(policy, _scope)| policy)
    }

    fn source_control_desc_at(offset: usize) -> Option<crate::global::ControlDesc> {
        let spec = <Steps as ProgramTerm>::PROGRAM_SOURCE
            .eff_list()
            .control_spec_at(offset)?;
        Some(crate::global::ControlDesc::from_static(spec).with_sites(
            crate::eff::EffIndex::from_dense_ordinal(offset),
            crate::global::ControlDesc::STATIC_POLICY_SITE,
        ))
    }

    const IMAGE: crate::global::compiled::lowering::CompiledProgramImage = {
        let source_data = <Steps as ProgramTerm>::PROGRAM_SOURCE;
        let source = source_data.eff_list();
        crate::global::compiled::lowering::CompiledProgramImage::scan_const_with_lookup(
            source,
            crate::global::compiled::lowering::ProgramSourceLookup::new(
                Self::source_policy_at,
                Self::source_control_desc_at,
            ),
        )
    };
}

const fn validate_program_projection<Steps>()
where
    Steps: ProgramTerm<Source = ProgramSourceData>,
{
    let source_data = <Steps as ProgramTerm>::PROGRAM_SOURCE;
    if let Some(error) = source_data.error {
        panic_program_source_error(error);
    }
    let source = source_data.eff_list();
    if let Some(error) =
        ProgramSourceError::from_policy_head_status(source.dynamic_policy_source_status())
    {
        panic_program_source_error(error);
    }
    ProgramProjection::<Steps>::IMAGE.validate_projection_program();
    if let Some(error) =
        crate::global::compiled::lowering::projection_error_all_roles(
            &ProgramProjection::<Steps>::IMAGE,
            source,
        )
    {
        panic_program_source_error(error);
    }
}

impl<Steps> Program<Steps> {
    #[inline(always)]
    pub(crate) fn validated_program_image()
    -> &'static crate::global::compiled::lowering::CompiledProgramImage
    where
        Steps: ProgramTerm<Source = ProgramSourceData>,
    {
        let _ = const { validate_program_projection::<Steps>() };
        &ProgramProjection::<Steps>::IMAGE
    }

    #[inline(always)]
    const fn compiled_program_image()
    -> &'static crate::global::compiled::lowering::CompiledProgramImage
    where
        Steps: ProgramTerm<Source = ProgramSourceData>,
    {
        &ProgramProjection::<Steps>::IMAGE
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn program_image(
        &self,
    ) -> &'static crate::global::compiled::lowering::CompiledProgramImage
    where
        Steps: ProgramTerm<Source = ProgramSourceData>,
    {
        let _ = self;
        Self::validated_program_image()
    }
}

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

use crate::global::LoopControlMeaning;
use crate::global::const_dsl::{EffList, PolicyMode, ScopeId, ScopeKind};
use crate::global::steps::RoleLaneMask;

pub(crate) trait ProgramTerm {
    type Source;
    const PROGRAM_SOURCE: Self::Source;
}

struct RoleProjection<const ROLE: u8, Steps>(PhantomData<Steps>);

impl<const ROLE: u8, Steps> RoleProjection<ROLE, Steps>
where
    Steps: ProgramTerm<Source = ProgramSourceData>,
{
    fn program_image() -> &'static crate::global::compiled::lowering::CompiledProgramImage {
        Program::<Steps>::compiled_program_image()
    }

    const STAMP: crate::global::compiled::lowering::ProgramStamp =
        ProgramProjection::<Steps>::IMAGE.stamp();
    const FACTS: crate::global::role_program::RoleFacts =
        crate::global::role_program::RoleFacts::from_counts(
            ProgramProjection::<Steps>::IMAGE.role_lowering_counts::<ROLE>(),
        );
    const LANES: crate::global::role_program::RoleLaneImage =
        crate::global::role_program::RoleLaneImage::from_program::<ROLE>(
            &ProgramProjection::<Steps>::IMAGE,
            Self::FACTS.footprint().logical_lane_count,
        );
    const ROLE_IMAGE: crate::global::role_program::RoleImage =
        crate::global::role_program::RoleImage::new(
            Self::FACTS,
            crate::global::role_program::RoleImageSource::new(Self::program_image),
            Self::LANES,
        );
    const IMAGE: crate::global::compiled::images::CompiledRoleImage =
        crate::global::compiled::images::CompiledRoleImage::new(
            crate::global::compiled::images::CompiledProgramRef::resident(
                Self::STAMP,
                &ProgramProjection::<Steps>::IMAGE,
            ),
            ROLE,
            crate::global::role_program::RoleImageRef::new(&Self::ROLE_IMAGE),
        );
}

#[inline(always)]
const fn role_projection_image<const ROLE: u8, Steps>()
-> &'static crate::global::compiled::images::CompiledRoleImage
where
    Steps: ProgramTerm<Source = ProgramSourceData>,
{
    &RoleProjection::<ROLE, Steps>::IMAGE
}

pub(crate) fn project_role<const ROLE: u8, Steps>(
    program: &Program<Steps>,
) -> crate::global::role_program::RoleProgram<ROLE>
where
    Steps: ProgramTerm<Source = ProgramSourceData>,
{
    crate::global::validate_role_index(ROLE);
    let _ = program;
    let _ = const { validate_program_projection::<Steps>() };
    crate::global::role_program::RoleProgram::new(role_projection_image::<ROLE, Steps>())
}

#[derive(Clone, Copy)]
pub(crate) struct ProgramSourceData {
    eff: EffList,
    role_lane_mask: RoleLaneMask,
    cycle_scope_pending: bool,
    tail_is_cycle_control: bool,
    error: Option<ProgramSourceError>,
}

#[derive(Clone, Copy)]
pub(crate) struct RouteHead {
    pub(crate) controller: u8,
    pub(crate) label: u8,
    pub(crate) cycle_meaning: Option<LoopControlMeaning>,
    pub(crate) error: Option<ProgramSourceError>,
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
            error: None,
        }
    }

    const fn merge_error(
        left: Option<ProgramSourceError>,
        right: Option<ProgramSourceError>,
    ) -> Option<ProgramSourceError> {
        if left.is_some() { left } else { right }
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
            return RouteHead {
                controller: 0,
                label: 0,
                cycle_meaning: None,
                error: Some(ProgramSourceError::RouteArmHead),
            };
        }
        let node = self.eff.node_at(0);
        if !matches!(node.kind, crate::eff::EffKind::Atom) {
            return RouteHead {
                controller: 0,
                label: 0,
                cycle_meaning: None,
                error: Some(ProgramSourceError::RouteArmHead),
            };
        }
        let atom = node.atom_data();
        if atom.from != atom.to {
            return RouteHead {
                controller: atom.from,
                label: atom.label,
                cycle_meaning: LoopControlMeaning::from_control_spec(self.eff.control_spec_at(0)),
                error: Some(ProgramSourceError::RouteArmHead),
            };
        }
        RouteHead {
            controller: atom.from,
            label: atom.label,
            cycle_meaning: LoopControlMeaning::from_control_spec(self.eff.control_spec_at(0)),
            error: None,
        }
    }

    pub(crate) const fn seq(self, next: Self) -> Self {
        let mut error = Self::merge_error(self.error, next.error);
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
                error = Self::merge_error(error, Some(ProgramSourceError::LoopBodyEmpty));
                eff = eff.extend_list(rebased);
            } else {
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
            }
        } else {
            eff = eff.extend_list(rebased);
            add_scope_budget(scope_budget, next.scope_budget());
        }
        Self {
            eff,
            role_lane_mask: self.role_lane_mask.union(next.role_lane_mask),
            cycle_scope_pending: false,
            tail_is_cycle_control: next_tail_is_cycle_control,
            error,
        }
    }

    pub(crate) const fn with_policy(self, policy_id: u16) -> Self {
        let mut error = self.error;
        if policy_id == crate::global::ControlDesc::STATIC_POLICY_SITE {
            error = Self::merge_error(error, Some(ProgramSourceError::PolicyIdReserved));
        }
        let eff = if self.eff.is_empty() {
            error = Self::merge_error(error, Some(ProgramSourceError::PolicyRequiresControlHead));
            self.eff
        } else {
            self.eff.with_policy(PolicyMode::dynamic(policy_id))
        };
        Self {
            eff,
            role_lane_mask: self.role_lane_mask,
            cycle_scope_pending: self.cycle_scope_pending,
            tail_is_cycle_control: self.tail_is_cycle_control,
            error,
        }
    }

    pub(crate) const fn route_with_controller(
        self,
        right: Self,
        controller: u8,
        is_cycle: bool,
        route_error: Option<ProgramSourceError>,
    ) -> Self {
        let mut error = Self::merge_error(self.error, right.error);
        error = Self::merge_error(error, route_error);
        error = Self::merge_error(
            error,
            ProgramSourceError::from_policy_head_status(
                self.eff.route_arm_dynamic_policy_head_status(),
            ),
        );
        error = Self::merge_error(
            error,
            ProgramSourceError::from_policy_head_status(
                right.eff.route_arm_dynamic_policy_head_status(),
            ),
        );
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
        let eff = if is_cycle {
            eff.with_scope_linger(scope, true)
        } else {
            eff
        };
        let cycle_scope_pending = eff.scope_has_linger(scope);
        Self {
            eff,
            role_lane_mask: self.role_lane_mask.union(right.role_lane_mask),
            cycle_scope_pending,
            tail_is_cycle_control: right.tail_is_cycle_control,
            error,
        }
    }

    pub(crate) const fn par(self, right: Self) -> Self {
        let mut error = Self::merge_error(self.error, right.error);
        if self.eff.is_empty() || right.eff.is_empty() {
            error = Self::merge_error(error, Some(ProgramSourceError::ParallelEmpty));
        }
        if self.role_lane_mask.intersects(&right.role_lane_mask) {
            error = Self::merge_error(error, Some(ProgramSourceError::ParallelConflict));
        }
        let parallel_scope = ScopeId::parallel(0);
        let left_budget = self.scope_budget();
        let right_offset = add_scope_budget(1, left_budget);
        let left_eff = self.into_eff().rebase_scopes(1);
        let right_eff = right.into_eff().rebase_scopes(right_offset);
        Self {
            eff: left_eff.extend_list(right_eff).with_scope(parallel_scope),
            role_lane_mask: self.role_lane_mask.union(right.role_lane_mask),
            cycle_scope_pending: false,
            tail_is_cycle_control: right.tail_is_cycle_control,
            error,
        }
    }
}

const fn add_scope_budget(lhs: u16, rhs: u16) -> u16 {
    let sum = lhs as u32 + rhs as u32;
    if sum > ScopeId::ORDINAL_CAPACITY as u32 {
        panic!("structured scope budget exceeded");
    }
    sum as u16
}
