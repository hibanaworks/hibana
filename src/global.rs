//! Global session type DSL (iso-recursive).
//!
//! This module exposes the primitives needed to assemble global choreographies
//! as local choreography witnesses and project them to role-local views.

use core::marker::PhantomData;

use self::program::Program;
use self::steps::{ParSteps, RouteSteps, SendStep, SeqSteps, StepCons, StepNil};
use crate::control::cap::mint::{ControlResourceKind, ResourceKind};
use crate::eff::EffIndex;

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
#[diagnostic::on_unimplemented(
    message = "`g::route(left, right)` arms must begin with a controller self-send",
    label = "route arm must begin with a controller self-send"
)]
pub(crate) trait RouteArmHead {
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
pub(crate) trait SameRouteControllerRole<Other> {}

pub(crate) const fn assert_distinct_route_labels<Left, Right>()
where
    Left: LabelTag,
    Right: LabelTag,
{
    if Left::VALUE == Right::VALUE {
        panic!("route arms reuse the same label");
    }
}

#[diagnostic::on_unimplemented(
    message = "`g::par(left, right)` arms must be non-empty protocol fragments",
    label = "parallel arm is empty"
)]
pub(crate) trait NonEmptyParallelArm {}

// -----------------------------------------------------------------------------
// Roles
// -----------------------------------------------------------------------------

pub(crate) const ROLE_DOMAIN_SIZE: usize = 16;

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

/// Canonical message descriptor when the label is known as a const generic.
pub type Msg<const LABEL: u8, Payload, Control = ()> =
    Message<LabelMarker<LABEL>, Payload, Control>;

fn encode_control_handle_for<K>(
    sid: crate::substrate::ids::SessionId,
    lane: crate::substrate::ids::Lane,
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
            crate::substrate::ids::SessionId,
            crate::substrate::ids::Lane,
            const_dsl::ScopeId,
        ) -> [u8; crate::control::cap::mint::CAP_HANDLE_LEN],
    >;
}

impl ControlPayloadKind for () {
    type ResourceKind = ();
    const IS_CONTROL: bool = false;
    const ENCODE_CONTROL_HANDLE: Option<
        fn(
            crate::substrate::ids::SessionId,
            crate::substrate::ids::Lane,
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
            crate::substrate::ids::SessionId,
            crate::substrate::ids::Lane,
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

pub(crate) const fn validate_role_index(role: u8) {
    if role >= ROLE_DOMAIN_SIZE as u8 {
        panic!("role index must be < 16");
    }
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
    const LOOP_MEANING: Option<LoopControlMeaning> = LoopControlMeaning::from_control_spec(
        <Message<LabelMarker<LABEL>, Payload, Control> as MessageControlSpec>::CONTROL,
    );
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
impl<Controller> SameRouteControllerRole<Controller> for Controller where Controller: RoleMarker {}

#[diagnostic::do_not_recommend]
impl<Head, Tail> NonEmptyParallelArm for StepCons<Head, Tail> {}

#[diagnostic::do_not_recommend]
impl<Left, Right> NonEmptyParallelArm for SeqSteps<Left, Right> where Left: NonEmptyParallelArm {}

#[diagnostic::do_not_recommend]
impl<Right> NonEmptyParallelArm for SeqSteps<StepNil, Right> where Right: NonEmptyParallelArm {}

#[diagnostic::do_not_recommend]
impl<Left, Right> NonEmptyParallelArm for steps::RouteSteps<Left, Right> where
    Left: NonEmptyParallelArm
{
}

#[diagnostic::do_not_recommend]
impl<Left, Right> NonEmptyParallelArm for steps::ParSteps<Left, Right> where
    Left: NonEmptyParallelArm
{
}

#[diagnostic::do_not_recommend]
impl<Inner, const POLICY_ID: u16> NonEmptyParallelArm for steps::PolicySteps<Inner, POLICY_ID> where
    Inner: NonEmptyParallelArm
{
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
        LoopControlMeaning::from_control_spec(<Msg as MessageControlSpec>::CONTROL).is_some()
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
    pub(crate) const fn of<K>() -> Self
    where
        K: ControlResourceKind,
    {
        if K::TAP_ID == 0 {
            panic!("control TAP_ID must be explicit");
        }
        Self {
            label: K::LABEL,
            resource_tag: K::TAG,
            scope_kind: K::SCOPE,
            path: K::PATH,
            tap_id: K::TAP_ID,
            shot: K::SHOT,
            op: K::OP,
            flags: if K::AUTO_MINT_WIRE { 1 } else { 0 },
        }
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

    pub(crate) const fn auto_mint_wire(self) -> bool {
        (self.flags & 1) != 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ControlDesc {
    eff_index: EffIndex,
    policy_site: u16,
    tap_id: u16,
    label: u8,
    resource_tag: u8,
    op: crate::control::cap::mint::ControlOp,
    scope_kind: const_dsl::ControlScopeKind,
    flags: u8,
}

impl ControlDesc {
    pub(crate) const STATIC_POLICY_SITE: u16 = u16::MAX;
    pub(crate) const EMPTY: Self = Self {
        eff_index: EffIndex::MAX,
        policy_site: Self::STATIC_POLICY_SITE,
        tap_id: 0,
        label: 0,
        resource_tag: 0,
        op: crate::control::cap::mint::ControlOp::Fence,
        scope_kind: const_dsl::ControlScopeKind::None,
        flags: 0,
    };
    const PATH_MASK: u8 = 0b0000_0001;
    const SHOT_MASK: u8 = 0b0000_0010;
    const AUTO_MINT_WIRE_MASK: u8 = 0b0000_0100;

    #[inline(always)]
    pub(crate) const fn of<K: ControlResourceKind>() -> Self {
        Self::from_static(StaticControlDesc::of::<K>())
    }

    #[inline(always)]
    pub(crate) const fn from_static(spec: StaticControlDesc) -> Self {
        Self::new(
            EffIndex::MAX,
            Self::STATIC_POLICY_SITE,
            spec.tap_id(),
            spec.label(),
            spec.resource_tag(),
            spec.op(),
            spec.scope_kind(),
            spec.path(),
            spec.shot(),
            spec.auto_mint_wire(),
        )
    }

    #[inline(always)]
    pub(crate) const fn with_sites(self, eff_index: EffIndex, policy_site: u16) -> Self {
        Self {
            eff_index,
            policy_site,
            tap_id: self.tap_id,
            label: self.label,
            resource_tag: self.resource_tag,
            op: self.op,
            scope_kind: self.scope_kind,
            flags: self.flags,
        }
    }

    #[inline(always)]
    pub(crate) const fn new(
        eff_index: EffIndex,
        policy_site: u16,
        tap_id: u16,
        label: u8,
        resource_tag: u8,
        op: crate::control::cap::mint::ControlOp,
        scope_kind: const_dsl::ControlScopeKind,
        path: crate::control::cap::mint::ControlPath,
        shot: crate::control::cap::mint::CapShot,
        auto_mint_wire: bool,
    ) -> Self {
        let mut flags = path.as_u8() & Self::PATH_MASK;
        if matches!(shot, crate::control::cap::mint::CapShot::Many) {
            flags |= Self::SHOT_MASK;
        }
        if auto_mint_wire {
            flags |= Self::AUTO_MINT_WIRE_MASK;
        }
        Self {
            eff_index,
            policy_site,
            tap_id,
            label,
            resource_tag,
            op,
            scope_kind,
            flags,
        }
    }

    #[inline(always)]
    pub(crate) const fn eff_index(self) -> EffIndex {
        self.eff_index
    }

    #[inline(always)]
    pub(crate) const fn policy_site(self) -> u16 {
        self.policy_site
    }

    #[inline(always)]
    pub(crate) const fn tap_id(self) -> u16 {
        self.tap_id
    }

    #[inline(always)]
    pub(crate) const fn label(self) -> u8 {
        self.label
    }

    #[inline(always)]
    pub(crate) const fn resource_tag(self) -> u8 {
        self.resource_tag
    }

    #[inline(always)]
    pub(crate) const fn op(self) -> crate::control::cap::mint::ControlOp {
        self.op
    }

    #[inline(always)]
    pub(crate) const fn scope_kind(self) -> const_dsl::ControlScopeKind {
        self.scope_kind
    }

    #[inline(always)]
    pub(crate) const fn path(self) -> crate::control::cap::mint::ControlPath {
        if (self.flags & Self::PATH_MASK) == 0 {
            crate::control::cap::mint::ControlPath::Local
        } else {
            crate::control::cap::mint::ControlPath::Wire
        }
    }

    #[inline(always)]
    pub(crate) const fn shot(self) -> crate::control::cap::mint::CapShot {
        if (self.flags & Self::SHOT_MASK) == 0 {
            crate::control::cap::mint::CapShot::One
        } else {
            crate::control::cap::mint::CapShot::Many
        }
    }

    #[inline(always)]
    pub(crate) const fn auto_mint_wire(self) -> bool {
        (self.flags & Self::AUTO_MINT_WIRE_MASK) != 0
    }

    #[inline(always)]
    pub(crate) const fn header_flags(self) -> u8 {
        if self.auto_mint_wire() { 1 } else { 0 }
    }

    #[inline(always)]
    pub(crate) const fn supports_dynamic_policy(self) -> bool {
        matches!(
            self.op(),
            crate::control::cap::mint::ControlOp::RouteDecision
                | crate::control::cap::mint::ControlOp::LoopContinue
                | crate::control::cap::mint::ControlOp::LoopBreak
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LoopControlMeaning {
    Continue,
    Break,
}

impl LoopControlMeaning {
    pub(crate) const fn from_control_desc(desc: Option<ControlDesc>) -> Option<Self> {
        match desc {
            Some(desc) => {
                if !matches!(desc.scope_kind(), const_dsl::ControlScopeKind::Loop) {
                    return None;
                }
                match desc.op() {
                    crate::control::cap::mint::ControlOp::LoopContinue => Some(Self::Continue),
                    crate::control::cap::mint::ControlOp::LoopBreak => Some(Self::Break),
                    _ => None,
                }
            }
            None => None,
        }
    }

    pub(crate) const fn from_control_spec(spec: Option<StaticControlDesc>) -> Option<Self> {
        match spec {
            Some(spec) => Self::from_control_desc(Some(ControlDesc::from_static(spec))),
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
}

const fn str_eq(lhs: &str, rhs: &str) -> bool {
    let lhs = lhs.as_bytes();
    let rhs = rhs.as_bytes();
    if lhs.len() != rhs.len() {
        return false;
    }
    let mut idx = 0usize;
    while idx < lhs.len() {
        if lhs[idx] != rhs[idx] {
            return false;
        }
        idx += 1;
    }
    true
}

const fn matches_builtin_control_kind<K, BuiltIn>() -> bool
where
    K: ControlResourceKind,
    BuiltIn: ControlResourceKind,
{
    K::TAG == BuiltIn::TAG
        && K::SCOPE as u8 == BuiltIn::SCOPE as u8
        && K::PATH as u8 == BuiltIn::PATH as u8
        && K::SHOT as u8 == BuiltIn::SHOT as u8
        && K::TAP_ID == BuiltIn::TAP_ID
        && K::OP as u8 == BuiltIn::OP as u8
        && str_eq(K::NAME, BuiltIn::NAME)
}

const fn validate_control_label_contract<const LABEL: u8, K>()
where
    K: ControlResourceKind,
{
    if LABEL != K::LABEL {
        panic!("control label mismatch");
    }
    match LABEL {
        crate::runtime::consts::LABEL_LOOP_CONTINUE => {
            if !matches_builtin_control_kind::<
                K,
                crate::control::cap::resource_kinds::LoopContinueKind,
            >() {
                panic!("core-owned control label is reserved");
            }
        }
        crate::runtime::consts::LABEL_LOOP_BREAK => {
            if !matches_builtin_control_kind::<K, crate::control::cap::resource_kinds::LoopBreakKind>(
            ) {
                panic!("core-owned control label is reserved");
            }
        }
        crate::runtime::consts::LABEL_ROUTE_DECISION => {
            if !matches_builtin_control_kind::<
                K,
                crate::control::cap::resource_kinds::RouteDecisionKind,
            >() {
                panic!("core-owned control label is reserved");
            }
        }
        _ if LABEL < crate::runtime::consts::LABEL_PROTOCOL_CONTROL_MIN => {
            panic!("control labels must stay inside the reserved protocol-control range");
        }
        _ => {}
    }
}

const fn validate_control_descriptor_contract(spec: StaticControlDesc) {
    match spec.op() {
        crate::control::cap::mint::ControlOp::CapDelegate => {
            panic!("cap-delegate control messages require the lower-layer endpoint token path");
        }
        crate::control::cap::mint::ControlOp::RouteDecision => {
            if !matches!(spec.scope_kind(), const_dsl::ControlScopeKind::Route) {
                panic!("route-decision control messages require route scope");
            }
            if !matches!(spec.path(), crate::control::cap::mint::ControlPath::Local) {
                panic!("route-decision control messages require local path");
            }
        }
        crate::control::cap::mint::ControlOp::LoopContinue
        | crate::control::cap::mint::ControlOp::LoopBreak => {
            if !matches!(spec.scope_kind(), const_dsl::ControlScopeKind::Loop) {
                panic!("loop control messages require loop scope");
            }
            if !matches!(spec.path(), crate::control::cap::mint::ControlPath::Local) {
                panic!("loop control messages require local path");
            }
        }
        _ => {}
    }
}

impl<L, P> MessageControlSpec for Message<L, P, ()>
where
    L: LabelTag,
    P: crate::transport::wire::WirePayload,
{
    const IS_CONTROL: bool = false;
    const CONTROL: Option<StaticControlDesc> = None;
}

impl<const LABEL: u8, K> MessageControlSpec
    for Message<LabelMarker<LABEL>, crate::control::cap::mint::GenericCapToken<K>, K>
where
    K: ControlResourceKind,
{
    const IS_CONTROL: bool = true;
    const CONTROL: Option<StaticControlDesc> = {
        validate_control_label_contract::<LABEL, K>();
        let spec = StaticControlDesc::of::<K>();
        validate_control_descriptor_contract(spec);
        Some(spec)
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
    From: KnownRole + RoleMarker,
    To: KnownRole + RoleMarker,
    M: MessageSpec + SendableLabel + MessageControlSpec,
{
    const {
        let from = <From as KnownRole>::INDEX;
        let to = <To as KnownRole>::INDEX;
        crate::global::validate_role_index(from);
        crate::global::validate_role_index(to);

        crate::global::validate_sendable_message::<M>();
        let is_control = <M as MessageControlSpec>::IS_CONTROL;

        if is_control {
            let is_self_send = from == to;
            let path = match <M as MessageControlSpec>::CONTROL {
                Some(desc) => desc.path(),
                None => panic!("control message missing descriptor"),
            };
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
    Program::new()
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
#[expect(
    private_bounds,
    reason = "route validation traits are internal compile-time witnesses"
)]
pub const fn route<LeftSteps, RightSteps>(
    left: Program<LeftSteps>,
    right: Program<RightSteps>,
) -> Program<RouteSteps<LeftSteps, RightSteps>>
where
    LeftSteps: RouteArmHead,
    RightSteps: RouteArmHead + TailLoopControl,
    <LeftSteps as RouteArmHead>::Controller:
        SameRouteControllerRole<<RightSteps as RouteArmHead>::Controller>,
{
    program::route_binary(left, right)
}

/// Construct a binary parallel composition.
#[expect(
    private_bounds,
    reason = "parallel validation traits are internal compile-time witnesses"
)]
pub const fn par<LeftSteps, RightSteps>(
    left: Program<LeftSteps>,
    right: Program<RightSteps>,
) -> Program<ParSteps<LeftSteps, RightSteps>>
where
    LeftSteps: program::BuildProgramSource + NonEmptyParallelArm,
    RightSteps: program::BuildProgramSource + NonEmptyParallelArm + TailLoopControl,
{
    program::par_binary(left, right)
}

#[cfg(test)]
mod tests {
    use super::{ControlDesc, Program, role_program::RoleProgram};
    use core::mem::size_of;

    #[test]
    fn descriptor_first_size_gates_hold() {
        assert_eq!(
            size_of::<Program<()>>(),
            0,
            "Program<Steps> must stay zero-sized"
        );
        assert!(
            size_of::<RoleProgram<0>>() <= 24,
            "RoleProgram<ROLE> must stay compact"
        );
        assert!(
            size_of::<ControlDesc>() <= 16,
            "ControlDesc must stay within the packed descriptor budget"
        );
    }
}
