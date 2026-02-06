//! Global session type DSL (iso-recursive).
//!
//! This module exposes the primitives needed to assemble global choreographies
//! as const values and project them to role-local views.

use core::marker::PhantomData;

use crate::control::cap::{ControlPayload, ControlResourceKind, ResourceKind};

/// Const-evaluated DSL and effect list plumbing.
pub mod const_dsl;
/// Control message definitions.
pub mod control_messages;
/// Program combinators and route builders.
pub mod program;
/// Role-local program projection and metadata.
pub mod role_program;
/// Type-level step combinators.
pub mod steps;
/// Typestate graph and cursor infrastructure.
pub mod typestate;

pub use const_dsl::{EffList, HandlePlan, StaticPlanKind};
pub use program::{
    LoopBreakSteps, LoopBreakStepsL, LoopContinueSteps, LoopContinueStepsL, LoopDecisionSteps,
    LoopDecisionStepsL, LoopSteps, LoopStepsL, ParChainBuilder, Program, RouteChainBuilder,
};
pub use role_program::{RoleProgram, project};
pub use steps::{LocalAction, LocalRecv, LocalSend, SendStep, StepConcat, StepCons, StepNil};
pub use typestate::{LoopMetadata, LoopRole, PhaseCursor, RoleTypestate};

// -----------------------------------------------------------------------------
// Roles
// -----------------------------------------------------------------------------

/// Compile-time role marker (0 ≤ IDX < 16).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Role<const IDX: u8>;

/// Marker trait exposing the numeric role index.
pub trait RoleMarker {
    const INDEX: u8;
}

impl<const IDX: u8> RoleMarker for Role<IDX> {
    const INDEX: u8 = IDX;
}

/// Trait implemented by every role type participating in a protocol.
pub trait KnownRole {
    const INDEX: u8;
}

impl<T: RoleMarker> KnownRole for T {
    const INDEX: u8 = T::INDEX;
}

// -----------------------------------------------------------------------------
// Labels & Messages
// -----------------------------------------------------------------------------

/// Marker trait for compile-time labels.
pub trait LabelTag {
    const VALUE: u8;
}

/// Concrete label marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LabelMarker<const LABEL: u8>;

impl<const LABEL: u8> LabelTag for LabelMarker<LABEL> {
    const VALUE: u8 = LABEL;
}

/// Phantom message descriptor tying a label to a payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Message<Label, Payload, Control = NoControl>(PhantomData<(Label, Payload, Control)>);

/// Type alias for convenience when the label is known as a const generic.
pub type Msg<const LABEL: u8, Payload, Control = NoControl> =
    Message<LabelMarker<LABEL>, Payload, Control>;

/// Handling strategy for control payloads.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlHandling {
    None = 0,
    Canonical = 1,
    External = 2,
}

/// Type-level description of how a control payload is produced.
pub trait ControlPayloadKind {
    type ResourceKind: ResourceKind;
    const HANDLING: ControlHandling;
}

/// Marker trait for control payload kinds that mint or import capability tokens.
pub trait ControlMessageKind: ControlPayloadKind {}

/// Marker indicating the message is not a control payload.
#[derive(Debug, Clone, Copy)]
pub struct NoControl;

impl ControlPayloadKind for NoControl {
    type ResourceKind = crate::control::cap::NoControlKind;
    const HANDLING: ControlHandling = ControlHandling::None;
}

/// Marker indicating the control payload must be minted locally.
#[derive(Debug, Clone, Copy)]
pub struct CanonicalControl<K: ResourceKind>(PhantomData<K>);

impl<K: ResourceKind> ControlPayloadKind for CanonicalControl<K> {
    type ResourceKind = K;
    const HANDLING: ControlHandling = ControlHandling::Canonical;
}

impl<K: ResourceKind> ControlMessageKind for CanonicalControl<K> {}

/// Marker trait enforcing that `CanonicalControl` messages require self-send (From == To).
///
/// This trait is only implemented for valid combinations:
/// - Any message with `NoControl` or `ExternalControl` (no self-send requirement)
/// - Messages with `CanonicalControl` only when `IsSelfSend = True`
///
/// Using `g::send::<A, B, Msg<..., CanonicalControl<K>>>` where `A ≠ B` will fail to compile
/// because this trait is not implemented for that combination.
pub trait RequireSelfSendForCanonical<IsSelfSend: steps::Bool> {}

// NoControl: always allowed (no self-send requirement)
impl<B: steps::Bool> RequireSelfSendForCanonical<B> for NoControl {}

// ExternalControl: always allowed (no self-send requirement)
impl<K: ResourceKind, B: steps::Bool> RequireSelfSendForCanonical<B> for ExternalControl<K> {}

// CanonicalControl: ONLY allowed when IsSelfSend = True
impl<K: ResourceKind> RequireSelfSendForCanonical<steps::True> for CanonicalControl<K> {}
// Note: No implementation for CanonicalControl<K> + False => compile error

/// Marker indicating the control payload is provided externally.
#[derive(Debug, Clone, Copy)]
pub struct ExternalControl<K: ResourceKind>(PhantomData<K>);

impl<K: ResourceKind> ControlPayloadKind for ExternalControl<K> {
    type ResourceKind = K;
    const HANDLING: ControlHandling = ControlHandling::External;
}

impl<K: ResourceKind> ControlMessageKind for ExternalControl<K> {}

/// Compile-time information carried with messages.
pub trait MessageSpec {
    /// Numeric label associated with the message.
    const LABEL: u8;
    /// Payload type transmitted on the wire.
    type Payload;
    /// Control payload handling strategy for this message.
    type ControlKind: ControlPayloadKind;
}

impl<L, P, C> MessageSpec for Message<L, P, C>
where
    L: LabelTag,
    C: ControlPayloadKind,
{
    const LABEL: u8 = L::VALUE;
    type Payload = P;
    type ControlKind = C;
}

/// Marker trait implemented by control-plane messages (canonical or external).
pub trait ControlMessage: MessageSpec {
    type ResourceKind: ControlResourceKind;
    const CONTROL_SPEC: ControlLabelSpec;
}

impl<L, P, C> ControlMessage for Message<L, P, C>
where
    L: LabelTag,
    C: ControlMessageKind,
    P: ControlPayload,
    <C as ControlPayloadKind>::ResourceKind: ControlResourceKind,
{
    type ResourceKind = <C as ControlPayloadKind>::ResourceKind;
    const CONTROL_SPEC: ControlLabelSpec = ControlLabelSpec::new(
        L::VALUE,
        <C as ControlPayloadKind>::ResourceKind::TAG,
        <C as ControlPayloadKind>::ResourceKind::SCOPE,
        <C as ControlPayloadKind>::ResourceKind::TAP_ID,
        <C as ControlPayloadKind>::ResourceKind::SHOT,
        <C as ControlPayloadKind>::HANDLING,
    );
}

/// Marker trait implemented for payloads permitted on control labels.
pub trait ControlLabelPayload<const LABEL: u8> {}

impl<const LABEL: u8, K> ControlLabelPayload<LABEL> for crate::control::cap::GenericCapToken<K> where
    K: ResourceKind
{
}

/// Marker trait for labels that may appear in outbound messages.
pub trait SendableLabel {
    const LABEL: u8;
    #[allow(clippy::empty_loop)]
    fn assert_sendable() {
        // Future work: enforce crash/no-send invariants here.
    }
}

impl<const LABEL: u8, Payload, Control> SendableLabel
    for Message<LabelMarker<LABEL>, Payload, Control>
where
    Message<LabelMarker<LABEL>, Payload, Control>: MessageControlSpec,
{
    const LABEL: u8 = LABEL;

    fn assert_sendable() {
        if LABEL > crate::runtime::consts::LABEL_MAX {
            panic!("label exceeds universe");
        }
        if LABEL >= crate::runtime::consts::LABEL_CONTROL_START
            && LABEL <= crate::runtime::consts::LABEL_CONTROL_END
            && !<Self as MessageControlSpec>::IS_CONTROL
        {
            panic!("control labels require capability payloads");
        }
    }
}

/// Static control-message metadata used across the DSL and runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ControlLabelSpec {
    pub label: u8,
    pub resource_tag: u8,
    pub scope_kind: const_dsl::ControlScopeKind,
    pub tap_id: u16,
    pub shot: crate::control::cap::CapShot,
    pub handling: ControlHandling,
}

impl ControlLabelSpec {
    pub const fn new(
        label: u8,
        resource_tag: u8,
        scope_kind: const_dsl::ControlScopeKind,
        tap_id: u16,
        shot: crate::control::cap::CapShot,
        handling: ControlHandling,
    ) -> Self {
        Self {
            label,
            resource_tag,
            scope_kind,
            tap_id,
            shot,
            handling,
        }
    }

    pub const fn from_message<L, K>(handling: ControlHandling) -> Self
    where
        L: LabelTag,
        K: ControlResourceKind,
    {
        if L::VALUE != K::LABEL {
            panic!("control label mismatch");
        }
        if K::HANDLING as u8 != handling as u8 {
            panic!("control handling mismatch");
        }
        Self::new(L::VALUE, K::TAG, K::SCOPE, K::TAP_ID, K::SHOT, handling)
    }
}

/// Per-message control metadata helper trait.
pub trait MessageControlSpec: MessageSpec {
    const IS_CONTROL: bool;
    const CONTROL_SPEC: ControlLabelSpec;
}

impl<L, P> MessageControlSpec for Message<L, P, NoControl>
where
    L: LabelTag,
{
    const IS_CONTROL: bool = false;
    const CONTROL_SPEC: ControlLabelSpec = ControlLabelSpec::new(
        L::VALUE,
        0,
        const_dsl::ControlScopeKind::None,
        0,
        crate::control::cap::CapShot::One,
        ControlHandling::None,
    );
}

impl<L, K> MessageControlSpec
    for Message<L, crate::control::cap::GenericCapToken<K>, CanonicalControl<K>>
where
    L: LabelTag,
    K: ControlResourceKind,
{
    const IS_CONTROL: bool = true;
    const CONTROL_SPEC: ControlLabelSpec =
        ControlLabelSpec::from_message::<L, K>(ControlHandling::Canonical);
}

impl<L, K> MessageControlSpec
    for Message<L, crate::control::cap::GenericCapToken<K>, ExternalControl<K>>
where
    L: LabelTag,
    K: ControlResourceKind,
{
    const IS_CONTROL: bool = true;
    const CONTROL_SPEC: ControlLabelSpec =
        ControlLabelSpec::from_message::<L, K>(ControlHandling::External);
}

/// Convenience alias exposing the projected local typelist for `Role`.
///
/// This resolves to the `ProjectRole` associated type and therefore fails to
/// compile whenever the requested role is absent from `Steps` or when payload /
/// label annotations do not line up.
pub type LocalProgram<Role, Steps> = <Steps as steps::ProjectRole<Role>>::Output;

// -----------------------------------------------------------------------------
// High-level combinators
// -----------------------------------------------------------------------------

/// Construct a single send step from `From` to `To` carrying `Msg` on `LANE`.
///
/// When using `g::par`, different Lanes allow the same roles to communicate
/// in parallel without violating the disjoint constraint (AMPST perspective).
///
/// # Examples
///
/// ```ignore
/// // Single lane communication
/// g::send::<Client, Server, Msg, 0>()
///
/// // Parallel composition with different Lanes (same roles)
/// g::par(
///     g::par_chain(g::send::<Client, Server, MsgA, 0>())
///         .and(g::send::<Server, Client, MsgB, 1>())
/// )
/// ```
pub const fn send<From, To, M, const LANE: u8>() -> Program<StepCons<SendStep<From, To, M, LANE>, StepNil>>
where
    From: KnownRole + RoleMarker + steps::RoleEq<To>,
    To: KnownRole + RoleMarker,
    M: MessageSpec + SendableLabel + MessageControlSpec,
    // Enforce: CanonicalControl requires self-send (From == To)
    <M as MessageSpec>::ControlKind:
        RequireSelfSendForCanonical<<From as steps::RoleEq<To>>::Output>,
{
    Program::build()
}

/// Empty protocol fragment.
pub const fn idle() -> Program<StepNil> {
    Program::empty()
}

/// Sequentially compose two protocol fragments.
pub const fn seq<LeftSteps, RightSteps>(
    left: Program<LeftSteps>,
    right: Program<RightSteps>,
) -> Program<<LeftSteps as StepConcat<RightSteps>>::Output>
where
    LeftSteps: StepConcat<RightSteps>,
{
    left.then(right)
}

/// Construct a route with two or more arms controlled by `Controller`.
///
/// Each arm must begin with a self-send from `CONTROLLER` (processed via
/// `flow().send()`). Other roles discover the selected arm via resolver or
/// [`poll_route_decision`].
///
/// [`poll_route_decision`]: crate::endpoint::CursorEndpoint::poll_route_decision
pub const fn route<const CONTROLLER: u8, Steps>(
    builder: program::RouteChainBuilder<CONTROLLER, Steps>,
) -> Program<Steps> {
    program::route::<CONTROLLER, Steps>(builder)
}

/// Begin building a route.
///
/// Each arm must begin with a self-send from `CONTROLLER`. This is enforced
/// at compile time by the [`RouteArm`] trait.
///
/// [`RouteArm`]: crate::global::steps::RouteArm
pub const fn route_chain<const CONTROLLER: u8, Steps>(
    arm: Program<Steps>,
) -> program::RouteChainBuilder<CONTROLLER, Steps>
where
    Steps: steps::RouteArm<CONTROLLER>,
{
    program::route_chain::<CONTROLLER, Steps>(arm)
}

/// Construct a parallel composition with two or more disjoint lanes.
pub const fn par<Steps>(builder: program::ParChainBuilder<Steps>) -> Program<Steps> {
    program::par(builder)
}

/// Begin building a parallel composition.
pub const fn par_chain<Steps>(lane: Program<Steps>) -> program::ParChainBuilder<Steps>
where
    Steps: steps::StepRoleSet + steps::StepNonEmpty,
{
    program::par_chain(lane)
}

/// Attach a control plan hint to a program fragment.
pub const fn with_control_plan<Steps>(program: Program<Steps>, plan: HandlePlan) -> Program<Steps>
where
    Steps: crate::global::steps::ControlPlanEligible,
{
    program.with_control_plan(plan)
}

/// Internal helpers exposed for tests and compile-fail fixtures.
pub mod __internal {
    /// Effect-list accumulator kept for compile-fail fixtures.
    pub use super::const_dsl::EffList;
}
