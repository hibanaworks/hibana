//! Global session type DSL (iso-recursive).
//!
//! This module exposes the primitives needed to assemble global choreographies
//! as local choreography witnesses and project them to role-local views.

use self::program::Program;
pub(crate) use self::types::ROLE_DOMAIN_SIZE;
pub use self::types::{KnownRole, RoleMarker};
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
/// Type-level step combinators.
pub(crate) mod steps;
/// Public role, label, and message marker types.
mod types;
/// Typestate graph and cursor infrastructure.
pub(crate) mod typestate;

mod message_seal {
    pub trait Sealed {}
}

fn encode_control_handle_for<K>(
    sid: crate::integration::ids::SessionId,
    lane: crate::integration::ids::Lane,
    scope: const_dsl::ScopeId,
) -> [u8; crate::control::cap::mint::CAP_HANDLE_LEN]
where
    K: ControlResourceKind,
{
    let handle = K::mint_handle(sid, lane, scope);
    K::encode_handle(&handle)
}

/// Type-level description of how a control payload is produced.
pub(crate) trait ControlPayloadKind {
    type ResourceKind: ResourceKind;
    const IS_CONTROL: bool;
    const ENCODE_CONTROL_HANDLE: Option<
        fn(
            crate::integration::ids::SessionId,
            crate::integration::ids::Lane,
            const_dsl::ScopeId,
        ) -> [u8; crate::control::cap::mint::CAP_HANDLE_LEN],
    >;
}

impl ControlPayloadKind for () {
    type ResourceKind = ();
    const IS_CONTROL: bool = false;
    const ENCODE_CONTROL_HANDLE: Option<
        fn(
            crate::integration::ids::SessionId,
            crate::integration::ids::Lane,
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
            crate::integration::ids::SessionId,
            crate::integration::ids::Lane,
            const_dsl::ScopeId,
        ) -> [u8; crate::control::cap::mint::CAP_HANDLE_LEN],
    > = Some(encode_control_handle_for::<K>);
}

/// Compile-time information carried with messages.
pub trait MessageSpec: message_seal::Sealed {
    /// Logical label associated with the choreography message.
    const LOGICAL_LABEL: u8;
    /// Payload type transmitted on the wire.
    type Payload: crate::transport::wire::WirePayload;
    /// Decoded payload view returned by `recv()` / `decode()`.
    type Decoded<'a>;
    /// Decode a payload that was already validated by the endpoint kernel.
    fn decode_validated_payload<'a>(
        input: crate::transport::wire::Payload<'a>,
    ) -> Self::Decoded<'a>;
    /// Opaque descriptor carrier for control messages.
    const CONTROL: Option<StaticControlDesc>;
    /// Whether the payload is a registered local control token.
    const CONTROL_PAYLOAD: bool;
    /// Encoder for the descriptor handle attached to a local control token.
    const ENCODE_CONTROL_HANDLE: Option<
        fn(
            crate::integration::ids::SessionId,
            crate::integration::ids::Lane,
            const_dsl::ScopeId,
        ) -> [u8; crate::control::cap::mint::CAP_HANDLE_LEN],
    >;
    /// Control payload kind for this message.
    type ControlKind;
}

impl<const LOGICAL_LABEL: u8, P, C> MessageSpec for crate::g::Msg<LOGICAL_LABEL, P, C>
where
    P: crate::transport::wire::WirePayload,
    C: ControlPayloadKind,
    crate::g::Msg<LOGICAL_LABEL, P, C>: MessageControlSpec,
{
    const LOGICAL_LABEL: u8 = LOGICAL_LABEL;
    type Payload = P;
    type Decoded<'a> = <P as crate::transport::wire::WirePayload>::Decoded<'a>;
    #[inline]
    fn decode_validated_payload<'a>(
        input: crate::transport::wire::Payload<'a>,
    ) -> Self::Decoded<'a> {
        <P as crate::transport::wire::WirePayload>::decode_validated_payload(input)
    }
    const CONTROL: Option<StaticControlDesc> = <Self as MessageControlSpec>::CONTROL;
    const CONTROL_PAYLOAD: bool = <C as ControlPayloadKind>::IS_CONTROL;
    const ENCODE_CONTROL_HANDLE: Option<
        fn(
            crate::integration::ids::SessionId,
            crate::integration::ids::Lane,
            const_dsl::ScopeId,
        ) -> [u8; crate::control::cap::mint::CAP_HANDLE_LEN],
    > = <C as ControlPayloadKind>::ENCODE_CONTROL_HANDLE;
    type ControlKind = C;
}

impl<const LOGICAL_LABEL: u8, P, C> message_seal::Sealed for crate::g::Msg<LOGICAL_LABEL, P, C>
where
    P: crate::transport::wire::WirePayload,
    C: ControlPayloadKind,
    crate::g::Msg<LOGICAL_LABEL, P, C>: MessageControlSpec,
{
}

/// Marker trait for labels that may appear in outbound messages.
pub trait SendableLabel {}

pub(crate) const fn validate_role_index(role: u8) {
    if role >= ROLE_DOMAIN_SIZE as u8 {
        panic!("role index must be < 16");
    }
}

impl<const SEND_LABEL: u8, Payload, Control> SendableLabel
    for crate::g::Msg<SEND_LABEL, Payload, Control>
where
    crate::g::Msg<SEND_LABEL, Payload, Control>: MessageControlSpec,
{
}

/// Static control-message metadata used across the DSL and runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StaticControlDesc {
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
            resource_tag: K::TAG,
            scope_kind: K::SCOPE,
            path: K::PATH,
            tap_id: K::TAP_ID,
            shot: K::SHOT,
            op: K::OP,
            flags: if K::AUTO_MINT_WIRE { 1 } else { 0 },
        }
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
    resource_tag: u8,
    op: crate::control::cap::mint::ControlOp,
    scope_kind: const_dsl::ControlScopeKind,
    flags: u8,
}

impl ControlDesc {
    pub(crate) const STATIC_POLICY_SITE: u16 = u16::MAX;
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
}

/// Per-message control metadata helper trait.
pub trait MessageControlSpec {
    const IS_CONTROL: bool;
    const CONTROL: Option<StaticControlDesc>;
}

const fn validate_control_descriptor_contract(spec: StaticControlDesc) {
    if spec.resource_tag() == 0 {
        panic!("control resource tag 0 is reserved");
    }
    match spec.op() {
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

const fn validate_unit_control_payload_contract(spec: StaticControlDesc) {
    validate_control_descriptor_contract(spec);
    if !matches!(spec.path(), crate::control::cap::mint::ControlPath::Local) {
        panic!("unit control payloads require local endpoint-owned controls");
    }
}

const fn validate_token_control_payload_contract(spec: StaticControlDesc) {
    validate_control_descriptor_contract(spec);
    if !matches!(spec.path(), crate::control::cap::mint::ControlPath::Wire) {
        panic!("GenericCapToken payloads require explicit wire controls");
    }
    if matches!(spec.shot(), crate::control::cap::mint::CapShot::One) {
        panic!("GenericCapToken wire controls require reusable descriptor semantics");
    }
    if spec.auto_mint_wire() {
        panic!("explicit GenericCapToken wire controls must set AUTO_MINT_WIRE=false");
    }
}

impl<const LOGICAL_LABEL: u8, P> MessageControlSpec for crate::g::Msg<LOGICAL_LABEL, P, ()>
where
    P: crate::transport::wire::WirePayload,
{
    const IS_CONTROL: bool = false;
    const CONTROL: Option<StaticControlDesc> = None;
}

impl<const LOGICAL_LABEL: u8, K> MessageControlSpec
    for crate::g::Msg<LOGICAL_LABEL, crate::control::cap::mint::GenericCapToken<K>, K>
where
    K: ControlResourceKind,
{
    const IS_CONTROL: bool = true;
    const CONTROL: Option<StaticControlDesc> = {
        let spec = StaticControlDesc::of::<K>();
        validate_token_control_payload_contract(spec);
        Some(spec)
    };
}

impl<const LOGICAL_LABEL: u8, K> MessageControlSpec for crate::g::Msg<LOGICAL_LABEL, (), K>
where
    K: ControlResourceKind,
{
    const IS_CONTROL: bool = true;
    const CONTROL: Option<StaticControlDesc> = {
        let spec = StaticControlDesc::of::<K>();
        validate_unit_control_payload_contract(spec);
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
pub const fn send<From, To, M, const LANE: u8>() -> Program<crate::g::Send<From, To, M, LANE>>
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
) -> Program<crate::g::Seq<LeftSteps, RightSteps>> {
    program::seq(left, right)
}

/// Construct a binary route.
///
/// The controller is derived from the first self-send control point in each arm.
/// Both arms must begin with the same controller self-send.
pub const fn route<LeftSteps, RightSteps>(
    left: Program<LeftSteps>,
    right: Program<RightSteps>,
) -> Program<crate::g::Route<LeftSteps, RightSteps>> {
    let _ = (left, right);
    Program::new()
}

/// Construct a binary parallel composition.
pub const fn par<LeftSteps, RightSteps>(
    left: Program<LeftSteps>,
    right: Program<RightSteps>,
) -> Program<crate::g::Par<LeftSteps, RightSteps>> {
    let _ = (left, right);
    Program::new()
}

#[cfg(all(test, hibana_repo_tests))]
mod tests;
