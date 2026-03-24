//! Global session type DSL (iso-recursive).
//!
//! This module exposes the primitives needed to assemble global choreographies
//! as const values and project them to role-local views.

use core::marker::PhantomData;

use self::program::Program;
use self::steps::{SendStep, SeqSteps, StepConcat, StepCons, StepNil};
use crate::control::cap::mint::{ControlPayload, ControlResourceKind, ResourceKind};

/// Const-evaluated DSL and effect list plumbing.
pub(crate) mod const_dsl;
/// Program combinators and route builders.
pub(crate) mod program;
/// Role-local program projection and metadata.
pub(crate) mod role_program;
/// Type-level step combinators.
pub(crate) mod steps;
/// Typestate graph and cursor infrastructure.
pub(crate) mod typestate;
/// Protocol-implementor compile-time SPI.
pub mod advanced {
    pub use super::role_program::{RoleProgram, project};
    pub use super::{
        CanonicalControl, ControlMessage, ControlMessageKind, ExternalControl, MessageSpec,
        const_dsl::EffList,
    };

    pub mod compose {
        pub use super::super::program::seq;
    }

    pub mod steps {
        pub use super::super::steps::{
            LocalAction, LocalRecv, LocalSend, LoopBreakSteps, LoopBreakStepsL, LoopContinueSteps,
            LoopContinueStepsL, LoopDecisionSteps, LoopDecisionStepsL, LoopSteps, LoopStepsL,
            ProjectRole, SendStep, SeqSteps, StepConcat, StepCons, StepNil,
        };
    }
}
#[diagnostic::on_unimplemented(
    message = "`g::route(left, right)` arms must begin with a controller self-send",
    label = "route arm must begin with a controller self-send"
)]
pub trait RouteArmHead {
    type Controller: RoleMarker;
    type Label: LabelTag;
}

#[diagnostic::on_unimplemented(
    message = "`g::route(left, right)` arms must start with the same controller self-send",
    label = "route arms use different controller self-sends"
)]
pub trait SameRouteController<Other> {}

#[diagnostic::on_unimplemented(
    message = "`g::route(left, right)` arms must use distinct labels",
    label = "route arms reuse the same label"
)]
pub trait DistinctRouteLabels<Other> {}

#[diagnostic::on_unimplemented(
    message = "`g::par(left, right)` arms must be non-empty protocol fragments",
    label = "parallel arm is empty"
)]
pub trait NonEmptyParallelArm {
    const ROLE_LANE_SET: steps::RoleLaneSet;
}

// -----------------------------------------------------------------------------
// Roles
// -----------------------------------------------------------------------------

/// Compile-time role marker (0 ≤ IDX < 16).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Role<const ROLE_INDEX: u8>;

/// Marker trait exposing the numeric role index.
pub trait RoleMarker {
    const INDEX: u8;
}

impl<const ROLE_INDEX: u8> RoleMarker for Role<ROLE_INDEX> {
    const INDEX: u8 = ROLE_INDEX;
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
pub struct LabelMarker<const LABEL_VALUE: u8>;

impl<const LABEL_VALUE: u8> LabelTag for LabelMarker<LABEL_VALUE> {
    const VALUE: u8 = LABEL_VALUE;
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
    type ResourceKind = ();
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

/// Marker trait for labels that may appear in outbound messages.
pub trait SendableLabel {
    const LABEL: u8;
    fn assert_sendable();
}

impl<const SEND_LABEL: u8, Payload, Control> SendableLabel
    for Message<LabelMarker<SEND_LABEL>, Payload, Control>
where
    Message<LabelMarker<SEND_LABEL>, Payload, Control>: MessageControlSpec,
{
    const LABEL: u8 = SEND_LABEL;

    fn assert_sendable() {
        if SEND_LABEL > crate::runtime::consts::LABEL_MAX {
            panic!("label exceeds universe");
        }
        if SEND_LABEL >= crate::runtime::consts::LABEL_CONTROL_START
            && SEND_LABEL <= crate::runtime::consts::LABEL_CONTROL_END
            && !<Self as MessageControlSpec>::IS_CONTROL
        {
            panic!("control labels require capability payloads");
        }
    }
}

trait LabelEq<Other> {
    type Output: steps::Bool;
}

#[diagnostic::on_unimplemented(
    message = "`g::route(left, right)` arms must use distinct labels",
    label = "route arms reuse the same label"
)]
trait RequireFalse {}

impl RequireFalse for steps::False {}

macro_rules! impl_label_eq {
    () => {};
    ($head:literal $(,$tail:literal)*) => {
        impl LabelEq<LabelMarker<$head>> for LabelMarker<$head> {
            type Output = steps::True;
        }
        $(
            impl LabelEq<LabelMarker<$tail>> for LabelMarker<$head> {
                type Output = steps::False;
            }

            impl LabelEq<LabelMarker<$head>> for LabelMarker<$tail> {
                type Output = steps::False;
            }
        )*
        impl_label_eq!($($tail),*);
    };
}

impl_label_eq!(
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25,
    26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49,
    50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 69, 70, 71, 72, 73,
    74, 75, 76, 77, 78, 79, 80, 81, 82, 83, 84, 85, 86, 87, 88, 89, 90, 91, 92, 93, 94, 95, 96, 97,
    98, 99, 100, 101, 102, 103, 104, 105, 106, 107, 108, 109, 110, 111, 112, 113, 114, 115, 116,
    117, 118, 119, 120, 121, 122, 123, 124, 125, 126, 127
);

#[diagnostic::do_not_recommend]
impl<RouteController, const LABEL: u8, Payload, Control, const LANE: u8, Tail> RouteArmHead
    for StepCons<
        SendStep<
            RouteController,
            RouteController,
            Message<LabelMarker<LABEL>, Payload, Control>,
            LANE,
        >,
        Tail,
    >
where
    RouteController: RoleMarker,
    Message<LabelMarker<LABEL>, Payload, Control>: MessageSpec + SendableLabel,
{
    type Controller = RouteController;
    type Label = LabelMarker<LABEL>;
}

#[diagnostic::do_not_recommend]
impl<Left, Right> RouteArmHead for SeqSteps<Left, Right>
where
    Left: RouteArmHead,
{
    type Controller = <Left as RouteArmHead>::Controller;
    type Label = <Left as RouteArmHead>::Label;
}

#[diagnostic::do_not_recommend]
impl<Right> RouteArmHead for SeqSteps<StepNil, Right>
where
    Right: RouteArmHead,
{
    type Controller = <Right as RouteArmHead>::Controller;
    type Label = <Right as RouteArmHead>::Label;
}

#[diagnostic::do_not_recommend]
impl<Left, Right, Controller> SameRouteController<Right> for Left
where
    Left: RouteArmHead<Controller = Controller>,
    Right: RouteArmHead<Controller = Controller>,
    Controller: RoleMarker,
{
}

#[diagnostic::do_not_recommend]
impl<Left> SameRouteController<StepNil> for Left where Left: RouteArmHead {}

#[diagnostic::do_not_recommend]
impl<Left, Right> DistinctRouteLabels<Right> for Left
where
    Left: RouteArmHead,
    Right: RouteArmHead,
    <Left as RouteArmHead>::Label: LabelEq<<Right as RouteArmHead>::Label>,
    <<Left as RouteArmHead>::Label as LabelEq<<Right as RouteArmHead>::Label>>::Output:
        RequireFalse,
{
}

#[diagnostic::do_not_recommend]
impl<Left> DistinctRouteLabels<StepNil> for Left where Left: RouteArmHead {}

#[diagnostic::do_not_recommend]
impl<Head, Tail> NonEmptyParallelArm for StepCons<Head, Tail>
where
    StepCons<Head, Tail>: steps::StepRoleSet,
{
    const ROLE_LANE_SET: steps::RoleLaneSet =
        <StepCons<Head, Tail> as steps::StepRoleSet>::ROLE_LANE_SET;
}

#[diagnostic::do_not_recommend]
impl<Left, Right> NonEmptyParallelArm for SeqSteps<Left, Right>
where
    Left: NonEmptyParallelArm,
    SeqSteps<Left, Right>: steps::StepRoleSet,
{
    const ROLE_LANE_SET: steps::RoleLaneSet =
        <SeqSteps<Left, Right> as steps::StepRoleSet>::ROLE_LANE_SET;
}

#[diagnostic::do_not_recommend]
impl<Right> NonEmptyParallelArm for SeqSteps<StepNil, Right>
where
    Right: NonEmptyParallelArm,
    SeqSteps<StepNil, Right>: steps::StepRoleSet,
{
    const ROLE_LANE_SET: steps::RoleLaneSet =
        <SeqSteps<StepNil, Right> as steps::StepRoleSet>::ROLE_LANE_SET;
}

/// Static control-message metadata used across the DSL and runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ControlLabelSpec {
    pub label: u8,
    pub resource_tag: u8,
    pub scope_kind: const_dsl::ControlScopeKind,
    pub tap_id: u16,
    pub shot: crate::control::cap::mint::CapShot,
    pub handling: ControlHandling,
}

impl ControlLabelSpec {
    pub const fn new(
        label: u8,
        resource_tag: u8,
        scope_kind: const_dsl::ControlScopeKind,
        tap_id: u16,
        shot: crate::control::cap::mint::CapShot,
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
        crate::control::cap::mint::CapShot::One,
        ControlHandling::None,
    );
}

impl<L, K> MessageControlSpec
    for Message<L, crate::control::cap::mint::GenericCapToken<K>, CanonicalControl<K>>
where
    L: LabelTag,
    K: ControlResourceKind,
{
    const IS_CONTROL: bool = true;
    const CONTROL_SPEC: ControlLabelSpec =
        ControlLabelSpec::from_message::<L, K>(ControlHandling::Canonical);
}

impl<L, K> MessageControlSpec
    for Message<L, crate::control::cap::mint::GenericCapToken<K>, ExternalControl<K>>
where
    L: LabelTag,
    K: ControlResourceKind,
{
    const IS_CONTROL: bool = true;
    const CONTROL_SPEC: ControlLabelSpec =
        ControlLabelSpec::from_message::<L, K>(ControlHandling::External);
}

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
///     g::send::<Client, Server, MsgA, 0>(),
///     g::send::<Server, Client, MsgB, 1>(),
/// )
/// ```
pub const fn send<From, To, M, const LANE: u8>()
-> Program<StepCons<SendStep<From, To, M, LANE>, StepNil>>
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

/// Sequentially compose two protocol fragments.
pub const fn seq<LeftSteps, RightSteps>(
    left: Program<LeftSteps>,
    right: Program<RightSteps>,
) -> Program<SeqSteps<LeftSteps, RightSteps>> {
    program::seq(left, right)
}

/// Construct a binary route.
///
/// The controller is derived from the first self-send control point in each arm.
/// Both arms must begin with the same controller self-send.
pub const fn route<LeftSteps, RightSteps>(
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
    program::route_binary(left, right)
}

/// Construct a binary parallel composition.
pub const fn par<LeftSteps, RightSteps>(
    left: Program<LeftSteps>,
    right: Program<RightSteps>,
) -> Program<<LeftSteps as StepConcat<RightSteps>>::Output>
where
    LeftSteps: StepConcat<RightSteps> + NonEmptyParallelArm,
    RightSteps: NonEmptyParallelArm,
{
    program::par_binary(left, right)
}
