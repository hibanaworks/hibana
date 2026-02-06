//! Typed program representation built from const DSL combinators.
//!
//! `Program<Steps>` pairs the value-level `EffList` with a type-level typelist
//! describing each global send. Projection uses this typelist to recover
//! payload types at compile time.

use core::marker::PhantomData;

use crate::control::cap::GenericCapToken;
use crate::control::cap::resource_kinds::{LoopBreakKind, LoopContinueKind};
use crate::eff;
use crate::g::{KnownRole, MessageSpec, RoleMarker};
use crate::global::const_dsl::{EffList, HandlePlan, ScopeId};
use crate::global::steps::{
    BuildEffList, ControlPlanEligible, RouteArm, SendStep, StepConcat, StepCons, StepNil,
    StepNonEmpty, StepRoleSet,
};
use crate::runtime::consts::{LABEL_LOOP_BREAK, LABEL_LOOP_CONTINUE};

/// Value + type-level representation of a global protocol fragment.
#[derive(Clone, Copy)]
pub struct Program<Steps> {
    eff: EffList,
    scope_budget: u16,
    loop_scope_pending: bool,
    steps: PhantomData<Steps>,
}

impl Program<StepNil> {
    pub const fn empty() -> Self {
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

    pub const fn build() -> Self
    where
        Steps: BuildEffList,
    {
        Self::wrap(Steps::EFF)
    }

    pub const fn eff_list(&self) -> &EffList {
        &self.eff
    }

    pub const fn into_eff(self) -> EffList {
        self.eff
    }

    pub const fn scope_budget(&self) -> u16 {
        self.scope_budget
    }

    pub const fn then<NextSteps>(
        self,
        next: Program<NextSteps>,
    ) -> Program<<Steps as StepConcat<NextSteps>>::Output>
    where
        Steps: StepConcat<NextSteps>,
    {
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
        Program {
            eff,
            scope_budget,
            loop_scope_pending: false,
            steps: PhantomData,
        }
    }
}

impl<Steps> Program<Steps>
where
    Steps: ControlPlanEligible,
{
    pub(crate) const fn with_control_plan(self, plan: HandlePlan) -> Self {
        Self::wrap(self.eff.with_control_plan(plan))
    }
}

const fn add_scope_budget(lhs: u16, rhs: u16) -> u16 {
    let sum = lhs as u32 + rhs as u32;
    if sum > ScopeId::ORDINAL_CAPACITY as u32 {
        panic!("structured scope budget exceeded");
    }
    sum as u16
}

/// Construct a single send step on a specific lane.
pub const fn send_program<From, To, M, const LANE: u8>() -> Program<StepCons<SendStep<From, To, M, LANE>, StepNil>>
where
    From: KnownRole + RoleMarker + crate::global::steps::RoleEq<To>,
    To: KnownRole + RoleMarker,
    M: crate::g::MessageSpec + crate::g::SendableLabel + crate::global::MessageControlSpec,
    // Enforce: CanonicalControl requires self-send (From == To)
    <M as crate::g::MessageSpec>::ControlKind: crate::global::RequireSelfSendForCanonical<
            <From as crate::global::steps::RoleEq<To>>::Output,
        >,
{
    Program::build()
}

// -----------------------------------------------------------------------------
// Route builder (chain-based, unbounded)
// -----------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct RouteChainBuilder<const CONTROLLER: u8, Steps> {
    eff: EffList,
    label_mask: u128,
    plan_mask: u128,
    /// Tracks arms that are self-send only (no cross-role messages).
    /// Bit 0 = arm 0, bit 1 = arm 1.
    arm_self_send_flags: u8,
    len: usize,
    scope: ScopeId,
    /// Scope ordinal cursor for disjoint arm ordinal allocation.
    /// Each arm's internal scopes get rebased to `scope_cursor`, then cursor advances
    /// by that arm's scope_budget. This ensures arms with internal scopes (loops, nested routes)
    /// don't collide on local ordinals.
    scope_cursor: u16,
    loop_detector: LoopRouteDetector,
    _steps: PhantomData<Steps>,
}

/// Start building a route with a single arm.
///
/// # Route Controller
///
/// The `CONTROLLER` role is the decision maker. Each arm must begin with a
/// self-send message (`send::<CONTROLLER, CONTROLLER, ControlMsg>`) that is
/// processed via `flow().send()` without traversing the wire.
///
/// Other roles discover the selected arm through one of these mechanisms:
/// - **Resolver**: Register a resolver via [`SessionCluster::register_control_plan_resolver`]
///   that returns the arm index. All roles can use this mechanism.
/// - **Poll**: Use [`poll_route_decision`] / [`ack_route_decision`] to synchronize
///   arm selection across roles within the same [`SessionCluster`].
///
/// # Dynamic Plans Require Resolvers
///
/// When using [`HandlePlan::dynamic`] for route arms, you **must** register a
/// resolver before executing the choreography. The resolver is called for each
/// role at the route decision point to determine the selected arm.
///
/// # Self-Send Enforcement
///
/// Route arms must begin with a self-send (`CONTROLLER → CONTROLLER`).
/// This is enforced at compile time by the [`RouteArm`] trait bound.
///
/// [`SessionCluster::register_control_plan_resolver`]: crate::control::cluster::SessionCluster::register_control_plan_resolver
/// [`poll_route_decision`]: crate::endpoint::CursorEndpoint::poll_route_decision
/// [`ack_route_decision`]: crate::endpoint::CursorEndpoint::ack_route_decision
/// [`SessionCluster`]: crate::control::cluster::SessionCluster
/// [`HandlePlan::dynamic`]: crate::global::const_dsl::HandlePlan::dynamic
/// [`RouteArm`]: crate::global::steps::RouteArm
pub const fn route_chain<const CONTROLLER: u8, Steps>(
    arm: Program<Steps>,
) -> RouteChainBuilder<CONTROLLER, Steps>
where
    Steps: RouteArm<CONTROLLER>,
{
    let scope = ScopeId::route(0);
    // Start cursor at 1 (route scope itself is ordinal 0)
    let inner_offset: u16 = 1;
    let arm_budget = arm.scope_budget();
    // Use with_scope_controller to propagate CONTROLLER role to ScopeMarker
    let eff = normalize_route_arm::<CONTROLLER>(
        arm.into_eff()
            .rebase_scopes(inner_offset)
            .with_scope_controller(scope, CONTROLLER),
    );
    let label = <<Steps as RouteArm<CONTROLLER>>::Msg as MessageSpec>::LABEL;
    let loop_detector = LoopRouteDetector::new().record::<CONTROLLER, Steps>();
    let bit = 1u128 << (label as u128);
    RouteChainBuilder {
        eff,
        label_mask: bit,
        plan_mask: plan_mask_for_route_arm(&eff, bit),
        arm_self_send_flags: if arm_is_self_send_only::<CONTROLLER>(&eff) {
            0b01
        } else {
            0
        },
        len: 1,
        scope,
        scope_cursor: add_scope_budget(inner_offset, arm_budget),
        loop_detector,
        _steps: PhantomData,
    }
}

impl<const CONTROLLER: u8, Steps> RouteChainBuilder<CONTROLLER, Steps> {
    /// Append another arm to the route.
    pub const fn and<NextSteps>(
        self,
        next: Program<NextSteps>,
    ) -> RouteChainBuilder<CONTROLLER, <Steps as StepConcat<NextSteps>>::Output>
    where
        Steps: StepConcat<NextSteps>,
        NextSteps: RouteArm<CONTROLLER>,
    {
        let RouteChainBuilder {
            eff,
            label_mask,
            plan_mask,
            arm_self_send_flags,
            len,
            scope,
            scope_cursor,
            loop_detector,
            _steps: _,
        } = self;
        // Each arm gets a disjoint ordinal range: rebase to current cursor, then advance
        let arm_budget = next.scope_budget();
        // Use with_scope_controller_role to propagate CONTROLLER to the scope markers.
        // We use with_scope first (for non-route inner scopes), then update controller_role.
        let mut next_eff = normalize_route_arm::<CONTROLLER>(
            next.into_eff()
                .rebase_scopes(scope_cursor)
                .with_scope(scope)
                .with_scope_controller_role(scope, CONTROLLER),
        );
        if eff.scope_has_linger(scope) {
            next_eff = next_eff.with_scope_linger(scope, true);
        }
        let label = <<NextSteps as RouteArm<CONTROLLER>>::Msg as MessageSpec>::LABEL;
        let bit = 1u128 << (label as u128);
        if (label_mask & bit) != 0 {
            panic!("duplicate route label");
        }
        let plan_bit = plan_mask_for_route_arm(&next_eff, bit);
        let self_send_bit = if arm_is_self_send_only::<CONTROLLER>(&next_eff) {
            1u8 << (len as u8)
        } else {
            0
        };

        RouteChainBuilder {
            eff: eff.extend_list(next_eff),
            label_mask: label_mask | bit,
            plan_mask: plan_mask | plan_bit,
            arm_self_send_flags: arm_self_send_flags | self_send_bit,
            len: len + 1,
            scope,
            scope_cursor: add_scope_budget(scope_cursor, arm_budget),
            loop_detector: loop_detector.record::<CONTROLLER, NextSteps>(),
            _steps: PhantomData,
        }
    }

    /// Mark this route scope as lingering until typestate exit.
    ///
    /// Note: For standard 2-arm loops (LoopContinue/LoopBreak), linger is applied
    /// automatically in `finish()`. This method is only needed for non-standard
    /// route patterns that require explicit linger behavior.
    pub(crate) const fn linger(mut self) -> Self {
        self.eff = self.eff.with_scope_linger(self.scope, true);
        self
    }

    /// Finalise the builder and materialise the route as a `Program`.
    ///
    /// # Control Plan Requirements
    ///
    /// Route arms beginning with self-send (`CONTROLLER → CONTROLLER`) do not require
    /// control plans. The controller makes a local decision, and receivers distinguish
    /// arms via label-based dispatch (`offer()` + `recv_label_hint()`).
    ///
    /// Note: The route builder validates unique labels per arm, ensuring receivers
    /// can always identify the selected arm without resolver coordination.
    ///
    /// # Auto-Linger for 2-Arm Loops
    ///
    /// Standard 2-arm loops (first arm starts with `LoopContinueMsg`, second with
    /// `LoopBreakMsg`) automatically have linger applied. This ensures the cursor
    /// returns to the route scope after the continue arm completes, enabling the
    /// next iteration. No manual `.linger()` call is needed for standard loops.
    pub const fn finish(self) -> Program<Steps> {
        if self.len != 2 {
            panic!("route must have exactly 2 arms (use nested routes for N-way branching)");
        }
        // Self-send routes don't require control plans - the controller makes a local
        // decision and receivers distinguish arms by label. The first step of each arm
        // is already validated as self-send in normalize_route_arm().
        let is_loop = self.loop_detector.is_loop(self.len);
        // Auto-linger for standard 2-arm loops (LoopContinue/LoopBreak pattern).
        // This removes the need for manual .linger() calls on loop routes.
        let eff = if is_loop {
            self.eff.with_scope_linger(self.scope, true)
        } else {
            self.eff
        };
        Program::wrap_with_hint(eff, is_loop)
    }
}

/// Convenience wrapper delegating to [`RouteChainBuilder::finish`].
pub const fn route<const CONTROLLER: u8, Steps>(
    builder: RouteChainBuilder<CONTROLLER, Steps>,
) -> Program<Steps> {
    builder.finish()
}

#[derive(Clone, Copy)]
struct LoopRouteDetector {
    mask: u8,
    invalid: bool,
}

impl LoopRouteDetector {
    const fn new() -> Self {
        Self {
            mask: 0,
            invalid: false,
        }
    }

    const fn record<const CONTROLLER: u8, Steps>(mut self) -> Self
    where
        Steps: RouteArm<CONTROLLER>,
    {
        let bit = classify_loop_arm::<CONTROLLER, Steps>();
        match bit {
            LoopArmKind::Continue => {
                if (self.mask & 0b01) != 0 {
                    self.invalid = true;
                } else {
                    self.mask |= 0b01;
                }
            }
            LoopArmKind::Break => {
                if (self.mask & 0b10) != 0 {
                    self.invalid = true;
                } else {
                    self.mask |= 0b10;
                }
            }
            LoopArmKind::None => {
                self.invalid = true;
            }
        }
        self
    }

    const fn is_loop(&self, len: usize) -> bool {
        !self.invalid && self.mask == 0b11 && len == 2
    }
}

enum LoopArmKind {
    Continue,
    Break,
    None,
}

const fn classify_loop_arm<const CONTROLLER: u8, Steps>() -> LoopArmKind
where
    Steps: RouteArm<CONTROLLER>,
{
    if <<Steps as RouteArm<CONTROLLER>>::Msg as MessageSpec>::LABEL == LABEL_LOOP_CONTINUE {
        LoopArmKind::Continue
    } else if <<Steps as RouteArm<CONTROLLER>>::Msg as MessageSpec>::LABEL == LABEL_LOOP_BREAK {
        LoopArmKind::Break
    } else {
        LoopArmKind::None
    }
}

// -----------------------------------------------------------------------------
// Par builder (chain-based, unbounded)
// -----------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct ParChainBuilder<Steps> {
    eff: EffList,
    /// Lane-aware role set for disjoint checking.
    /// From AMPST perspective, different Lanes are independent channels,
    /// so the same roles can communicate in parallel on different Lanes.
    role_lane_set: crate::global::steps::RoleLaneSet,
    len: usize,
    /// Shared scope identifier for the entire parallel region.
    parallel_scope: ScopeId,
    scope_cursor: u16,
    _steps: PhantomData<Steps>,
}

/// Start building a parallel composition with a single lane.
pub const fn par_chain<Steps>(lane: Program<Steps>) -> ParChainBuilder<Steps>
where
    Steps: StepRoleSet + StepNonEmpty,
{
    let role_lane_set = <Steps as StepRoleSet>::ROLE_LANE_SET;
    if role_lane_set.is_empty() {
        panic!("parallel lane must not be empty");
    }
    let lane_scope_budget = lane.scope_budget();
    let outer = 0;
    let parallel_scope = ScopeId::parallel(outer);
    let inner_offset = add_scope_budget(outer, 1);
    let eff = lane.into_eff().rebase_scopes(inner_offset);
    ParChainBuilder {
        eff,
        role_lane_set,
        len: 1,
        parallel_scope,
        scope_cursor: add_scope_budget(inner_offset, lane_scope_budget),
        _steps: PhantomData,
    }
}

impl<Steps> ParChainBuilder<Steps> {
    /// Append another lane to the parallel composition.
    ///
    /// From AMPST perspective, parallel lanes must use disjoint (role, lane) pairs.
    /// The same roles can communicate in parallel if they are on different Lanes.
    pub const fn and<NextSteps>(
        self,
        next: Program<NextSteps>,
    ) -> ParChainBuilder<<Steps as StepConcat<NextSteps>>::Output>
    where
        Steps: StepConcat<NextSteps>,
        NextSteps: StepRoleSet + StepNonEmpty,
    {
        let ParChainBuilder {
            eff,
            role_lane_set,
            len,
            parallel_scope,
            scope_cursor,
            _steps: _,
        } = self;

        let next_set = <NextSteps as StepRoleSet>::ROLE_LANE_SET;
        if next_set.is_empty() {
            panic!("parallel lane must not be empty");
        }
        // Lane-aware disjoint check: reject only if same (role, lane) pair overlaps
        if role_lane_set.intersects(&next_set) {
            panic!("parallel lanes must use disjoint (role, lane) pairs");
        }

        let lane_scope_budget = next.scope_budget();
        let inner_offset = scope_cursor;
        let rebased = next.into_eff().rebase_scopes(inner_offset);
        ParChainBuilder {
            eff: eff.extend_list(rebased),
            role_lane_set: role_lane_set.union(next_set),
            len: len + 1,
            parallel_scope,
            scope_cursor: add_scope_budget(inner_offset, lane_scope_budget),
            _steps: PhantomData,
        }
    }

    /// Finalise the builder and materialise the parallel program.
    pub const fn finish(self) -> Program<Steps> {
        if self.len < 2 {
            panic!("parallel composition requires at least two lanes");
        }
        Program::wrap(self.eff.with_scope(self.parallel_scope))
    }
}

/// Convenience wrapper delegating to [`ParChainBuilder::finish`].
pub const fn par<Steps>(builder: ParChainBuilder<Steps>) -> Program<Steps> {
    builder.finish()
}

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------

const fn normalize_route_arm<const CONTROLLER: u8>(arm: EffList) -> EffList {
    if arm.is_empty() {
        panic!("route arm must not be empty");
    }
    let atom = arm.first_atom();
    // The first step must be a self-send from CONTROLLER.
    // This is the hibana design: route decisions are made via local self-send
    // control messages processed by flow().send().
    if atom.from != CONTROLLER {
        panic!("route arms must begin with controller send");
    }
    if atom.to != CONTROLLER {
        panic!("route arm first step must be self-send (CONTROLLER → CONTROLLER)");
    }
    if !matches!(atom.direction, eff::EffDirection::Send) {
        panic!("route arms must begin with send atoms");
    }
    arm
}

const fn plan_mask_for_route_arm(arm: &EffList, bit: u128) -> u128 {
    if route_arm_has_control_plan(arm) {
        bit
    } else {
        0
    }
}

const fn route_arm_has_control_plan(arm: &EffList) -> bool {
    match arm.control_plan_at(0) {
        Some(plan) => !plan.is_none(),
        None => false,
    }
}

/// Returns true if all Send atoms in the arm are self-send (from == to == CONTROLLER).
const fn arm_is_self_send_only<const CONTROLLER: u8>(arm: &EffList) -> bool {
    let mut idx = 0;
    while idx < arm.len() {
        let node = arm.at(idx);
        if matches!(node.kind, eff::EffKind::Atom) {
            let atom = node.data.atom();
            // Check if this is a Send atom (not Recv or Local)
            if matches!(atom.direction, eff::EffDirection::Send) {
                // If it's a cross-role send, this arm is NOT self-send only
                if atom.from != atom.to || atom.from != CONTROLLER {
                    return false;
                }
            }
        }
        idx += 1;
    }
    true
}

/// Loop continue steps using self-send (Controller → Controller) for canonical control.
///
/// CanonicalControl messages are local-only decisions that should not be sent on wire.
/// By using self-send, the Controller executes a LocalAction::Local while the decision
/// is transparent to other roles.
pub type LoopContinueSteps<Controller, ContMsg, Tail = StepNil> =
    StepCons<SendStep<Controller, Controller, ContMsg>, Tail>;

/// Loop break steps using self-send (Controller → Controller) for canonical control.
///
/// See `LoopContinueSteps` for rationale.
pub type LoopBreakSteps<Controller, BreakMsg, Tail = StepNil> =
    StepCons<SendStep<Controller, Controller, BreakMsg>, Tail>;

/// Loop continue steps on a specific lane.
///
/// Like `LoopContinueSteps` but with explicit lane specification for use in
/// parallel compositions where the same controller operates on multiple lanes.
pub type LoopContinueStepsL<Controller, ContMsg, const LANE: u8, Tail = StepNil> =
    StepCons<SendStep<Controller, Controller, ContMsg, LANE>, Tail>;

/// Loop break steps on a specific lane.
///
/// Like `LoopBreakSteps` but with explicit lane specification.
pub type LoopBreakStepsL<Controller, BreakMsg, const LANE: u8, Tail = StepNil> =
    StepCons<SendStep<Controller, Controller, BreakMsg, LANE>, Tail>;

pub type LoopDecisionSteps<Controller, ContMsg, BreakMsg, BreakTail = StepNil, ContTail = StepNil> =
    <LoopContinueSteps<Controller, ContMsg, ContTail> as StepConcat<
        LoopBreakSteps<Controller, BreakMsg, BreakTail>,
    >>::Output;

/// Loop decision steps on a specific lane.
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

pub type LoopSteps<
    BodySteps,
    Controller,
    ContMsg,
    BreakMsg,
    BreakTail = StepNil,
    ContTail = StepNil,
> = <LoopDecisionSteps<
    Controller,
    ContMsg,
    BreakMsg,
    BreakTail,
    <BodySteps as StepConcat<ContTail>>::Output,
> as LoopStepsAssert<ContMsg, BreakMsg>>::Output;

/// Loop steps on a specific lane.
pub type LoopStepsL<
    BodySteps,
    Controller,
    ContMsg,
    BreakMsg,
    const LANE: u8,
    BreakTail = StepNil,
    ContTail = StepNil,
> = <LoopDecisionStepsL<
    Controller,
    ContMsg,
    BreakMsg,
    LANE,
    BreakTail,
    <BodySteps as StepConcat<ContTail>>::Output,
> as LoopStepsAssert<ContMsg, BreakMsg>>::Output;

pub trait LoopStepsAssert<ContMsg, BreakMsg> {
    type Output;
}

impl<ContMsg, BreakMsg, Steps> LoopStepsAssert<ContMsg, BreakMsg> for Steps
where
    ContMsg: LoopContinueMessage,
    BreakMsg: LoopBreakMessage,
{
    type Output = Steps;
}

pub trait LoopContinueMessage:
    crate::g::ControlMessage + crate::global::MessageControlSpec
{
}

impl LoopContinueMessage
    for crate::g::Msg<
        { LABEL_LOOP_CONTINUE },
        GenericCapToken<LoopContinueKind>,
        crate::g::CanonicalControl<LoopContinueKind>,
    >
{
}

pub trait LoopBreakMessage: crate::g::ControlMessage + crate::global::MessageControlSpec {}

impl LoopBreakMessage
    for crate::g::Msg<
        { LABEL_LOOP_BREAK },
        GenericCapToken<LoopBreakKind>,
        crate::g::CanonicalControl<LoopBreakKind>,
    >
{
}
