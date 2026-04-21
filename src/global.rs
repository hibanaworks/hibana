//! Global session type DSL (iso-recursive).
//!
//! This module exposes the primitives needed to assemble global choreographies
//! as local inferred witnesses and project them to role-local views.

use core::marker::PhantomData;

use self::program::Program;
use self::steps::{ParSteps, RouteSteps, SendStep, SeqSteps, StepCons, StepNil};
use crate::control::cap::mint::{ControlResourceKind, ResourceKind};

/// Crate-private lowering owners for unified compilation.
pub(crate) mod compiled;
/// Const-evaluated DSL and effect list plumbing.
pub(crate) mod const_dsl;
/// Program combinators and route builders.
pub(crate) mod program;
/// Role-local program projection and metadata.
pub(crate) mod role_program;
pub(crate) use role_program::RoleProgramView;
#[cfg(test)]
pub(crate) use role_program::lowering_input;
/// Type-level step combinators.
pub(crate) mod steps;
/// Typestate graph and cursor infrastructure.
pub(crate) mod typestate;
/// Protocol-implementor compile-time SPI.
pub mod advanced {
    pub use super::role_program::{RoleProgram, project};
    pub use super::{MessageSpec, StaticControlDesc};
}
#[diagnostic::on_unimplemented(
    message = "`g::route(left, right)` arms must begin with a controller self-send",
    label = "route arm must begin with a controller self-send"
)]
pub trait RouteArmHead {
    type Controller: RoleMarker;
    type Label: LabelTag;
}

pub(crate) trait RouteArmLoopHead {
    const LOOP_MEANING: Option<LoopControlMeaning>;
}

pub(crate) trait TailLoopControl {
    const IS_LOOP_CONTROL: bool;
}

pub(crate) trait FragmentShape {
    const IS_EMPTY: bool;
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
pub struct Message<Label, Payload, Control = ()>(PhantomData<(Label, Payload, Control)>);

/// Type alias for convenience when the label is known as a const generic.
pub type Msg<const LABEL: u8, Payload, Control = ()> =
    Message<LabelMarker<LABEL>, Payload, Control>;

fn encode_control_handle_for<K>(
    sid: crate::substrate::SessionId,
    lane: crate::substrate::Lane,
    scope: const_dsl::ScopeId,
) -> [u8; crate::control::cap::mint::CAP_HANDLE_LEN]
where
    K: ControlResourceKind,
{
    let handle = K::mint_handle(sid, lane, scope);
    K::encode_handle(&handle)
}

/// Type-level description of how a control payload is produced.
pub trait ControlPayloadKind {
    type ResourceKind: ResourceKind;
    const IS_CONTROL: bool;
    const ENCODE_CONTROL_HANDLE: Option<
        fn(
            crate::substrate::SessionId,
            crate::substrate::Lane,
            const_dsl::ScopeId,
        ) -> [u8; crate::control::cap::mint::CAP_HANDLE_LEN],
    >;
}

impl ControlPayloadKind for () {
    type ResourceKind = ();
    const IS_CONTROL: bool = false;
    const ENCODE_CONTROL_HANDLE: Option<
        fn(
            crate::substrate::SessionId,
            crate::substrate::Lane,
            const_dsl::ScopeId,
        ) -> [u8; crate::control::cap::mint::CAP_HANDLE_LEN],
    > = None;
}

impl<K> ControlPayloadKind for K
where
    K: ControlResourceKind,
{
    type ResourceKind = K;
    const IS_CONTROL: bool = true;
    const ENCODE_CONTROL_HANDLE: Option<
        fn(
            crate::substrate::SessionId,
            crate::substrate::Lane,
            const_dsl::ScopeId,
        ) -> [u8; crate::control::cap::mint::CAP_HANDLE_LEN],
    > = Some(encode_control_handle_for::<K>);
}

/// Compile-time information carried with messages.
pub trait MessageSpec {
    /// Numeric label associated with the message.
    const LABEL: u8;
    /// Payload type transmitted on the wire.
    type Payload;
    /// Decoded payload view returned by `recv()` / `decode()`.
    type Decoded<'a>;
    /// Opaque descriptor carrier for control messages.
    const CONTROL: Option<StaticControlDesc>;
    /// Control payload kind for this message.
    type ControlKind: ControlPayloadKind;
}

impl<L, P, C> MessageSpec for Message<L, P, C>
where
    L: LabelTag,
    P: crate::transport::wire::WirePayload,
    C: ControlPayloadKind,
    Message<L, P, C>: MessageControlSpec,
{
    const LABEL: u8 = L::VALUE;
    type Payload = P;
    type Decoded<'a> = <P as crate::transport::wire::WirePayload>::Decoded<'a>;
    const CONTROL: Option<StaticControlDesc> = <Self as MessageControlSpec>::CONTROL;
    type ControlKind = C;
}

/// Marker trait for labels that may appear in outbound messages.
pub trait SendableLabel {
    const LABEL: u8;
    fn assert_sendable();
}

pub(crate) const fn validate_sendable_message<M>()
where
    M: MessageSpec + MessageControlSpec,
{
    let label = <M as MessageSpec>::LABEL;
    if label > crate::runtime::consts::LABEL_MAX {
        panic!("label exceeds universe");
    }
    if !<M as MessageControlSpec>::IS_CONTROL
        && crate::global::const_dsl::is_reserved_control_label(label)
    {
        panic!("control labels require capability payloads");
    }
}

impl<const SEND_LABEL: u8, Payload, Control> SendableLabel
    for Message<LabelMarker<SEND_LABEL>, Payload, Control>
where
    Message<LabelMarker<SEND_LABEL>, Payload, Control>: MessageControlSpec,
{
    const LABEL: u8 = SEND_LABEL;

    fn assert_sendable() {
        crate::global::validate_sendable_message::<Self>();
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

mod label_eq;

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
impl<RouteController, const LABEL: u8, Payload, Control, const LANE: u8, Tail> RouteArmLoopHead
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
    Message<LabelMarker<LABEL>, Payload, Control>: MessageSpec + MessageControlSpec + SendableLabel,
{
    const LOOP_MEANING: Option<LoopControlMeaning> = LoopControlMeaning::from_control_spec(Some(
        <Message<LabelMarker<LABEL>, Payload, Control> as MessageControlSpec>::CONTROL_SPEC,
    ));
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
impl<Left, Right> RouteArmLoopHead for SeqSteps<Left, Right>
where
    Left: RouteArmLoopHead,
{
    const LOOP_MEANING: Option<LoopControlMeaning> = <Left as RouteArmLoopHead>::LOOP_MEANING;
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
impl<Right> RouteArmLoopHead for SeqSteps<StepNil, Right>
where
    Right: RouteArmLoopHead,
{
    const LOOP_MEANING: Option<LoopControlMeaning> = <Right as RouteArmLoopHead>::LOOP_MEANING;
}

#[diagnostic::do_not_recommend]
impl<Inner, const POLICY_ID: u16> RouteArmHead for steps::PolicySteps<Inner, POLICY_ID>
where
    Inner: RouteArmHead,
{
    type Controller = <Inner as RouteArmHead>::Controller;
    type Label = <Inner as RouteArmHead>::Label;
}

#[diagnostic::do_not_recommend]
impl<Inner, const POLICY_ID: u16> RouteArmLoopHead for steps::PolicySteps<Inner, POLICY_ID>
where
    Inner: RouteArmLoopHead,
{
    const LOOP_MEANING: Option<LoopControlMeaning> = <Inner as RouteArmLoopHead>::LOOP_MEANING;
}

#[diagnostic::do_not_recommend]
impl<Left, Right> RouteArmHead for steps::RouteSteps<Left, Right>
where
    Left: RouteArmHead,
{
    type Controller = <Left as RouteArmHead>::Controller;
    type Label = <Left as RouteArmHead>::Label;
}

#[diagnostic::do_not_recommend]
impl<Left, Right> RouteArmLoopHead for steps::RouteSteps<Left, Right>
where
    Left: RouteArmLoopHead,
{
    const LOOP_MEANING: Option<LoopControlMeaning> = <Left as RouteArmLoopHead>::LOOP_MEANING;
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

#[diagnostic::do_not_recommend]
impl<Left, Right> NonEmptyParallelArm for steps::RouteSteps<Left, Right>
where
    Left: NonEmptyParallelArm,
    steps::RouteSteps<Left, Right>: steps::StepRoleSet,
{
    const ROLE_LANE_SET: steps::RoleLaneSet =
        <steps::RouteSteps<Left, Right> as steps::StepRoleSet>::ROLE_LANE_SET;
}

#[diagnostic::do_not_recommend]
impl<Left, Right> NonEmptyParallelArm for steps::ParSteps<Left, Right>
where
    Left: NonEmptyParallelArm,
    steps::ParSteps<Left, Right>: steps::StepRoleSet,
{
    const ROLE_LANE_SET: steps::RoleLaneSet =
        <steps::ParSteps<Left, Right> as steps::StepRoleSet>::ROLE_LANE_SET;
}

#[diagnostic::do_not_recommend]
impl<Inner, const POLICY_ID: u16> NonEmptyParallelArm for steps::PolicySteps<Inner, POLICY_ID>
where
    Inner: NonEmptyParallelArm,
    steps::PolicySteps<Inner, POLICY_ID>: steps::StepRoleSet,
{
    const ROLE_LANE_SET: steps::RoleLaneSet =
        <steps::PolicySteps<Inner, POLICY_ID> as steps::StepRoleSet>::ROLE_LANE_SET;
}

#[diagnostic::do_not_recommend]
impl FragmentShape for StepNil {
    const IS_EMPTY: bool = true;
}

#[diagnostic::do_not_recommend]
impl<From, To, Msg, const LANE: u8, Tail> FragmentShape
    for StepCons<SendStep<From, To, Msg, LANE>, Tail>
{
    const IS_EMPTY: bool = false;
}

#[diagnostic::do_not_recommend]
impl<Left, Right> FragmentShape for SeqSteps<Left, Right>
where
    Left: FragmentShape,
    Right: FragmentShape,
{
    const IS_EMPTY: bool = <Left as FragmentShape>::IS_EMPTY && <Right as FragmentShape>::IS_EMPTY;
}

#[diagnostic::do_not_recommend]
impl<Left, Right> FragmentShape for steps::RouteSteps<Left, Right>
where
    Left: FragmentShape,
    Right: FragmentShape,
{
    const IS_EMPTY: bool = <Left as FragmentShape>::IS_EMPTY && <Right as FragmentShape>::IS_EMPTY;
}

#[diagnostic::do_not_recommend]
impl<Left, Right> FragmentShape for steps::ParSteps<Left, Right>
where
    Left: FragmentShape,
    Right: FragmentShape,
{
    const IS_EMPTY: bool = <Left as FragmentShape>::IS_EMPTY && <Right as FragmentShape>::IS_EMPTY;
}

#[diagnostic::do_not_recommend]
impl<Inner, const POLICY_ID: u16> FragmentShape for steps::PolicySteps<Inner, POLICY_ID>
where
    Inner: FragmentShape,
{
    const IS_EMPTY: bool = <Inner as FragmentShape>::IS_EMPTY;
}

#[diagnostic::do_not_recommend]
impl TailLoopControl for StepNil {
    const IS_LOOP_CONTROL: bool = false;
}

#[diagnostic::do_not_recommend]
impl<From, To, Msg, const LANE: u8, Tail> TailLoopControl
    for StepCons<SendStep<From, To, Msg, LANE>, Tail>
where
    Msg: MessageSpec + MessageControlSpec,
    Tail: FragmentShape + TailLoopControl,
{
    const IS_LOOP_CONTROL: bool = if <Tail as FragmentShape>::IS_EMPTY {
        LoopControlMeaning::from_control_spec(Some(<Msg as MessageControlSpec>::CONTROL_SPEC))
            .is_some()
    } else {
        <Tail as TailLoopControl>::IS_LOOP_CONTROL
    };
}

#[diagnostic::do_not_recommend]
impl<Left, Right> TailLoopControl for SeqSteps<Left, Right>
where
    Left: TailLoopControl,
    Right: FragmentShape + TailLoopControl,
{
    const IS_LOOP_CONTROL: bool = if <Right as FragmentShape>::IS_EMPTY {
        <Left as TailLoopControl>::IS_LOOP_CONTROL
    } else {
        <Right as TailLoopControl>::IS_LOOP_CONTROL
    };
}

#[diagnostic::do_not_recommend]
impl<Left, Right> TailLoopControl for steps::RouteSteps<Left, Right>
where
    Right: TailLoopControl,
{
    const IS_LOOP_CONTROL: bool = <Right as TailLoopControl>::IS_LOOP_CONTROL;
}

#[diagnostic::do_not_recommend]
impl<Left, Right> TailLoopControl for steps::ParSteps<Left, Right>
where
    Right: TailLoopControl,
{
    const IS_LOOP_CONTROL: bool = <Right as TailLoopControl>::IS_LOOP_CONTROL;
}

#[diagnostic::do_not_recommend]
impl<Inner, const POLICY_ID: u16> TailLoopControl for steps::PolicySteps<Inner, POLICY_ID>
where
    Inner: TailLoopControl,
{
    const IS_LOOP_CONTROL: bool = <Inner as TailLoopControl>::IS_LOOP_CONTROL;
}

/// Static control-message metadata used across the DSL and runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StaticControlDesc {
    label: u8,
    resource_tag: u8,
    scope_kind: const_dsl::ControlScopeKind,
    path: crate::control::cap::mint::ControlPath,
    tap_id: u16,
    shot: crate::control::cap::mint::CapShot,
    op: crate::control::cap::mint::ControlOp,
    flags: u8,
}

impl StaticControlDesc {
    pub(crate) const fn new(
        label: u8,
        resource_tag: u8,
        scope_kind: const_dsl::ControlScopeKind,
        path: crate::control::cap::mint::ControlPath,
        tap_id: u16,
        shot: crate::control::cap::mint::CapShot,
        op: crate::control::cap::mint::ControlOp,
        flags: u8,
    ) -> Self {
        Self {
            label,
            resource_tag,
            scope_kind,
            path,
            tap_id,
            shot,
            op,
            flags,
        }
    }

    pub(crate) const fn of<K>() -> Self
    where
        K: ControlResourceKind,
    {
        Self::new(
            K::LABEL,
            K::TAG,
            K::SCOPE,
            K::PATH,
            K::TAP_ID,
            K::SHOT,
            K::OP,
            if K::AUTO_MINT_WIRE { 1 } else { 0 },
        )
    }

    pub(crate) const fn label(self) -> u8 {
        self.label
    }

    pub(crate) const fn resource_tag(self) -> u8 {
        self.resource_tag
    }

    pub(crate) const fn scope_kind(self) -> const_dsl::ControlScopeKind {
        self.scope_kind
    }

    pub(crate) const fn tap_id(self) -> u16 {
        self.tap_id
    }

    pub(crate) const fn path(self) -> crate::control::cap::mint::ControlPath {
        self.path
    }

    pub(crate) const fn shot(self) -> crate::control::cap::mint::CapShot {
        self.shot
    }

    pub(crate) const fn op(self) -> crate::control::cap::mint::ControlOp {
        self.op
    }

    pub(crate) const fn supports_dynamic_policy(self) -> bool {
        matches!(
            self.op(),
            crate::control::cap::mint::ControlOp::RouteDecision
                | crate::control::cap::mint::ControlOp::LoopContinue
                | crate::control::cap::mint::ControlOp::LoopBreak
                | crate::control::cap::mint::ControlOp::TopologyBegin
                | crate::control::cap::mint::ControlOp::TopologyAck
                | crate::control::cap::mint::ControlOp::CapDelegate
        )
    }

    pub(crate) const fn auto_mint_wire(self) -> bool {
        (self.flags & 1) != 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LoopControlMeaning {
    Continue,
    Break,
}

impl LoopControlMeaning {
    pub(crate) const fn from_control_spec(spec: Option<StaticControlDesc>) -> Option<Self> {
        match spec {
            Some(spec) => {
                if !matches!(spec.scope_kind(), const_dsl::ControlScopeKind::Loop) {
                    return None;
                }
                match spec.op() {
                    crate::control::cap::mint::ControlOp::LoopContinue => Some(Self::Continue),
                    crate::control::cap::mint::ControlOp::LoopBreak => Some(Self::Break),
                    _ => None,
                }
            }
            None => None,
        }
    }

    pub(crate) const fn from_semantic(
        semantic: crate::global::compiled::images::ControlSemanticKind,
    ) -> Option<Self> {
        match semantic {
            crate::global::compiled::images::ControlSemanticKind::LoopContinue => {
                Some(Self::Continue)
            }
            crate::global::compiled::images::ControlSemanticKind::LoopBreak => Some(Self::Break),
            _ => None,
        }
    }

    pub(crate) const fn arm(self) -> u8 {
        match self {
            Self::Continue => 0,
            Self::Break => 1,
        }
    }
}

/// Per-message control metadata helper trait.
pub trait MessageControlSpec: MessageSpec {
    const IS_CONTROL: bool;
    const CONTROL: Option<StaticControlDesc>;
    const CONTROL_SPEC: StaticControlDesc;
}

struct ControlLabelContract<const LABEL: u8, K>(PhantomData<fn() -> K>);

trait ValidControlLabel {}

impl<K: ControlResourceKind> ValidControlLabel for ControlLabelContract<106, K> {}
impl<K: ControlResourceKind> ValidControlLabel for ControlLabelContract<107, K> {}
impl<K: ControlResourceKind> ValidControlLabel for ControlLabelContract<108, K> {}
impl<K: ControlResourceKind> ValidControlLabel for ControlLabelContract<109, K> {}
impl<K: ControlResourceKind> ValidControlLabel for ControlLabelContract<110, K> {}
impl<K: ControlResourceKind> ValidControlLabel for ControlLabelContract<111, K> {}
impl<K: ControlResourceKind> ValidControlLabel for ControlLabelContract<112, K> {}
impl<K: ControlResourceKind> ValidControlLabel for ControlLabelContract<113, K> {}
impl<K: ControlResourceKind> ValidControlLabel for ControlLabelContract<114, K> {}
impl<K: ControlResourceKind> ValidControlLabel for ControlLabelContract<115, K> {}
impl<K: ControlResourceKind> ValidControlLabel for ControlLabelContract<116, K> {}
impl<K: ControlResourceKind> ValidControlLabel for ControlLabelContract<117, K> {}
impl<K: ControlResourceKind> ValidControlLabel for ControlLabelContract<118, K> {}
impl<K: ControlResourceKind> ValidControlLabel for ControlLabelContract<119, K> {}
impl<K: ControlResourceKind> ValidControlLabel for ControlLabelContract<120, K> {}
impl<K: ControlResourceKind> ValidControlLabel for ControlLabelContract<121, K> {}
impl<K: ControlResourceKind> ValidControlLabel for ControlLabelContract<122, K> {}
impl<K: ControlResourceKind> ValidControlLabel for ControlLabelContract<123, K> {}
impl<K: ControlResourceKind> ValidControlLabel for ControlLabelContract<124, K> {}
impl<K: ControlResourceKind> ValidControlLabel for ControlLabelContract<125, K> {}
impl<K: ControlResourceKind> ValidControlLabel for ControlLabelContract<126, K> {}
impl<K: ControlResourceKind> ValidControlLabel for ControlLabelContract<127, K> {}

impl ValidControlLabel
    for ControlLabelContract<
        { crate::runtime::consts::LABEL_LOOP_CONTINUE },
        crate::control::cap::resource_kinds::LoopContinueKind,
    >
{
}

impl ValidControlLabel
    for ControlLabelContract<
        { crate::runtime::consts::LABEL_LOOP_BREAK },
        crate::control::cap::resource_kinds::LoopBreakKind,
    >
{
}

impl ValidControlLabel
    for ControlLabelContract<
        { crate::runtime::consts::LABEL_ROUTE_DECISION },
        crate::control::cap::resource_kinds::RouteDecisionKind,
    >
{
}

impl<L, P> MessageControlSpec for Message<L, P, ()>
where
    L: LabelTag,
    P: crate::transport::wire::WirePayload,
{
    const IS_CONTROL: bool = false;
    const CONTROL: Option<StaticControlDesc> = None;
    const CONTROL_SPEC: StaticControlDesc = StaticControlDesc::new(
        L::VALUE,
        0,
        const_dsl::ControlScopeKind::None,
        crate::control::cap::mint::ControlPath::Local,
        0,
        crate::control::cap::mint::CapShot::One,
        crate::control::cap::mint::ControlOp::Fence,
        0,
    );
}

impl<const LABEL: u8, K> MessageControlSpec
    for Message<LabelMarker<LABEL>, crate::control::cap::mint::GenericCapToken<K>, K>
where
    K: ControlResourceKind,
    ControlLabelContract<LABEL, K>: ValidControlLabel,
{
    const IS_CONTROL: bool = true;
    const CONTROL: Option<StaticControlDesc> = {
        if LABEL != K::LABEL {
            panic!("control label mismatch");
        }
        Some(StaticControlDesc::of::<K>())
    };
    const CONTROL_SPEC: StaticControlDesc = {
        if LABEL != K::LABEL {
            panic!("control label mismatch");
        }
        StaticControlDesc::of::<K>()
    };
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
{
    const {
        crate::global::validate_sendable_message::<M>();
        if <M as MessageControlSpec>::IS_CONTROL {
            let is_self_send = <<From as steps::RoleEq<To>>::Output as steps::Bool>::VALUE;
            let path = <M as MessageControlSpec>::CONTROL_SPEC.path();
            match path {
                crate::control::cap::mint::ControlPath::Local if !is_self_send => {
                    panic!("local control messages require self-send")
                }
                crate::control::cap::mint::ControlPath::Wire if is_self_send => {
                    panic!("wire control messages require cross-role send")
                }
                _ => {}
            }
        }
    }
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
#[allow(private_bounds)]
pub const fn route<LeftSteps, RightSteps>(
    left: Program<LeftSteps>,
    right: Program<RightSteps>,
) -> Program<RouteSteps<LeftSteps, RightSteps>>
where
    LeftSteps: RouteArmHead + SameRouteController<RightSteps> + DistinctRouteLabels<RightSteps>,
    RightSteps: RouteArmHead + TailLoopControl,
{
    program::route_binary(left, right)
}

/// Construct a binary parallel composition.
#[allow(private_bounds)]
pub const fn par<LeftSteps, RightSteps>(
    left: Program<LeftSteps>,
    right: Program<RightSteps>,
) -> Program<ParSteps<LeftSteps, RightSteps>>
where
    LeftSteps: NonEmptyParallelArm,
    RightSteps: NonEmptyParallelArm + TailLoopControl,
{
    program::par_binary(left, right)
}
