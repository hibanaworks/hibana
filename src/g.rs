//! Choreography language used by app authors.
//!
//! `g` is the only app-facing language layer. Build local choreography terms
//! with [`send`], [`seq`], [`route`], and [`par`], then let a protocol crate
//! project and attach them.
//!
//! ```rust,ignore
//! use hibana::g;
//!
//! let request = g::send::<0, 1, g::Msg<1, u32>>();
//! let reply = g::send::<1, 0, g::Msg<2, u32>>();
//! let program = g::seq(request, reply);
//! ```
//!
//! A [`Msg`] is a typed message descriptor:
//!
//! ```text
//! Msg<LOGICAL_LABEL, Payload>
//! ```
//!
//! Labels identify choreography messages and route branches. They do not encode
//! transport demux or control semantics.
//!
//! Dynamic branch policy is supplied by integration resolvers. Runtime hints or
//! payload contents do not create route authority by themselves.

mod source;
mod terms;

use core::marker::PhantomData;

use crate::control::cap::mint::{ControlOp, ControlPath};
use crate::global::{MessageRuntime, StaticControlDesc};

pub use crate::global::Message;
pub(crate) use source::{ProgramSourceData, ProgramTerm};

pub(crate) const ROLE_DOMAIN_SIZE: u8 = 16;
const ROLE_INDEX_ERROR: &str = "role index must be < 16";

mod role_projection;

/// Canonical message descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Msg<const LOGICAL_LABEL: u8, Payload>(PhantomData<Payload>);

/// Control protocol event descriptor.
///
/// `ControlMsg` is carried by [`send`] like any other choreography event, but it
/// lowers to Hibana control rows instead of an application payload row. `Kind`
/// is one of the sealed marker types in [`control`]; callers cannot define new
/// control descriptors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ControlMsg<const LOGICAL_LABEL: u8, Kind>(PhantomData<Kind>);

const _: ControlMsg<0, ()> = ControlMsg(PhantomData);

/// Built-in control protocol event kinds.
///
/// These marker types are intentionally closed. They name protocol events that
/// Hibana can lower into control metadata without exposing raw descriptor
/// construction or capability-token layout.
pub mod control {
    use crate::control::{
        cap::{
            atomic_codecs::SessionLaneHandle,
            mint::{CAP_HANDLE_LEN, CapShot, ControlOp, LocalControlKind},
            resource_kinds::LoopDecisionHandle,
        },
        types::{Lane, SessionId},
    };
    use crate::global::const_dsl::{ControlScopeKind, ScopeId};
    use crate::observe::ids;

    /// Local loop-continue protocol event.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct LoopContinue;

    /// Local loop-break protocol event.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct LoopBreak;

    /// Local state-snapshot protocol event.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct StateSnapshot;

    /// Local state-restore protocol event.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct StateRestore;

    /// Local transaction-commit protocol event.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct TxnCommit;

    /// Local transaction-abort protocol event.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct TxnAbort;

    /// Distributed topology-begin protocol event.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct TopologyBegin;

    /// Distributed topology-ack protocol event.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct TopologyAck;

    /// Distributed topology-commit protocol event.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct TopologyCommit;

    fn encode_session_lane_handle(sid: SessionId, lane: Lane) -> [u8; CAP_HANDLE_LEN] {
        SessionLaneHandle::new(sid.raw(), lane.as_wire() as u16).to_bytes()
    }

    impl LocalControlKind for LoopContinue {
        const TAG: u8 = 0x40;
        const SCOPE: ControlScopeKind = ControlScopeKind::Loop;
        const TAP_ID: u16 = ids::LOOP_DECISION;
        const SHOT: CapShot = CapShot::One;
        const OP: ControlOp = ControlOp::LoopContinue;

        fn encode_local_handle(
            sid: SessionId,
            lane: Lane,
            _scope: ScopeId,
        ) -> [u8; CAP_HANDLE_LEN] {
            LoopDecisionHandle::new(sid.raw(), lane.as_wire()).encode()
        }
    }

    impl LocalControlKind for LoopBreak {
        const TAG: u8 = 0x41;
        const SCOPE: ControlScopeKind = ControlScopeKind::Loop;
        const TAP_ID: u16 = ids::LOOP_DECISION;
        const SHOT: CapShot = CapShot::One;
        const OP: ControlOp = ControlOp::LoopBreak;

        fn encode_local_handle(
            sid: SessionId,
            lane: Lane,
            _scope: ScopeId,
        ) -> [u8; CAP_HANDLE_LEN] {
            LoopDecisionHandle::new(sid.raw(), lane.as_wire()).encode()
        }
    }

    impl LocalControlKind for StateSnapshot {
        const TAG: u8 = 0x42;
        const SCOPE: ControlScopeKind = ControlScopeKind::State;
        const TAP_ID: u16 = ids::STATE_SNAPSHOT_REQ;
        const SHOT: CapShot = CapShot::One;
        const OP: ControlOp = ControlOp::StateSnapshot;

        fn encode_local_handle(
            sid: SessionId,
            lane: Lane,
            _scope: ScopeId,
        ) -> [u8; CAP_HANDLE_LEN] {
            encode_session_lane_handle(sid, lane)
        }
    }

    impl LocalControlKind for StateRestore {
        const TAG: u8 = 0x43;
        const SCOPE: ControlScopeKind = ControlScopeKind::State;
        const TAP_ID: u16 = ids::STATE_RESTORE_REQ;
        const SHOT: CapShot = CapShot::One;
        const OP: ControlOp = ControlOp::StateRestore;

        fn encode_local_handle(
            sid: SessionId,
            lane: Lane,
            _scope: ScopeId,
        ) -> [u8; CAP_HANDLE_LEN] {
            encode_session_lane_handle(sid, lane)
        }
    }

    impl LocalControlKind for TxnCommit {
        const TAG: u8 = 0x44;
        const SCOPE: ControlScopeKind = ControlScopeKind::State;
        const TAP_ID: u16 = ids::POLICY_COMMIT;
        const SHOT: CapShot = CapShot::One;
        const OP: ControlOp = ControlOp::TxCommit;

        fn encode_local_handle(
            sid: SessionId,
            lane: Lane,
            _scope: ScopeId,
        ) -> [u8; CAP_HANDLE_LEN] {
            encode_session_lane_handle(sid, lane)
        }
    }

    impl LocalControlKind for TxnAbort {
        const TAG: u8 = 0x45;
        const SCOPE: ControlScopeKind = ControlScopeKind::State;
        const TAP_ID: u16 = ids::POLICY_TX_ABORT;
        const SHOT: CapShot = CapShot::One;
        const OP: ControlOp = ControlOp::TxAbort;

        fn encode_local_handle(
            sid: SessionId,
            lane: Lane,
            _scope: ScopeId,
        ) -> [u8; CAP_HANDLE_LEN] {
            encode_session_lane_handle(sid, lane)
        }
    }
}

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
    ResolverIdReserved,
    ResolverTargetNotRoute,
    ResolverUnsupportedControlSite,
    ProjectionRoutePolicyMismatch,
    ProjectionRoutePolicyMissing,
    ProjectionRouteUnprojectable,
}

impl ProgramSourceError {
    pub(crate) const fn from_dynamic_resolver_source_status(status: u8) -> Option<Self> {
        match status {
            0 => None,
            1 => Some(Self::ResolverTargetNotRoute),
            3 => Some(Self::ResolverUnsupportedControlSite),
            2 | 4..=u8::MAX => crate::invariant(),
        }
    }
}

pub(crate) const fn panic_choreography_error(error: ProgramSourceError) -> ! {
    match error as u8 {
        0 => panic!("g::route arms must begin with a visible action"),
        1 => panic!("route arms reuse the same label"),
        2 => panic!("route arms use different first visible controllers"),
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
            panic!("route resolver id u16::MAX is reserved")
        }
        9 => {
            panic!("route resolver can only be attached to a route")
        }
        10 => panic!("route resolver site is not supported"),
        11 => panic!("route resolver mismatch"),
        12 => panic!("route resolver missing"),
        13 => panic!(concat!(
            "Route unprojectable for this role: arms not mergeable, ",
            "wire dispatch non-deterministic, ",
            "and no route resolver provided",
        )),
        14..=u8::MAX => crate::invariant(),
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

#[derive(Clone, Copy)]
#[repr(u8)]
pub(crate) enum MessageControlContractError {
    MissingDescriptor,
    DescriptorTagReserved,
    LoopScope,
    LoopPath,
    ControlPathMismatch,
}

impl MessageControlContractError {
    pub(crate) const fn message(self) -> &'static str {
        match self {
            Self::MissingDescriptor => "control message missing descriptor",
            Self::DescriptorTagReserved => "control descriptor tag 0 is reserved",
            Self::LoopScope => "loop control messages require loop scope",
            Self::LoopPath => "loop control messages require local path",
            Self::ControlPathMismatch => "control descriptor path does not match message roles",
        }
    }
}

const fn control_descriptor_contract_error(
    spec: StaticControlDesc,
) -> Option<MessageControlContractError> {
    if spec.resource_tag() == 0 {
        return Some(MessageControlContractError::DescriptorTagReserved);
    }
    if matches!(spec.op(), ControlOp::LoopContinue | ControlOp::LoopBreak) {
        if !matches!(
            spec.scope_kind(),
            crate::global::const_dsl::ControlScopeKind::Loop
        ) {
            return Some(MessageControlContractError::LoopScope);
        }
        if !matches!(spec.path(), ControlPath::Local) {
            return Some(MessageControlContractError::LoopPath);
        }
    }
    None
}

const fn unit_control_payload_contract_error(
    spec: StaticControlDesc,
) -> Option<MessageControlContractError> {
    if let Some(error) = control_descriptor_contract_error(spec) {
        return Some(error);
    }
    if !matches!(spec.path(), ControlPath::Local) {
        return Some(MessageControlContractError::ControlPathMismatch);
    }
    None
}

const fn unit_wire_control_payload_contract_error(
    spec: StaticControlDesc,
) -> Option<MessageControlContractError> {
    if let Some(error) = control_descriptor_contract_error(spec) {
        return Some(error);
    }
    if !matches!(spec.path(), ControlPath::Wire) {
        return Some(MessageControlContractError::ControlPathMismatch);
    }
    None
}

pub(crate) const fn role_pair_contract_error<const FROM: u8, const TO: u8>() -> Option<&'static str>
{
    if FROM >= ROLE_DOMAIN_SIZE || TO >= ROLE_DOMAIN_SIZE {
        return Some(ROLE_INDEX_ERROR);
    }
    None
}

pub(crate) const fn message_control_contract_error<M>() -> Option<MessageControlContractError>
where
    M: Message,
{
    if !<M as MessageRuntime>::CONTROL_PAYLOAD {
        return None;
    }
    let Some(spec) = StaticControlDesc::from_runtime_tuple(<M as MessageRuntime>::CONTROL) else {
        return Some(MessageControlContractError::MissingDescriptor);
    };
    match <M as MessageRuntime>::CONTROL_PAYLOAD_KIND {
        crate::global::CONTROL_PAYLOAD_LOCAL_UNIT => unit_control_payload_contract_error(spec),
        crate::global::CONTROL_PAYLOAD_WIRE_UNIT => unit_wire_control_payload_contract_error(spec),
        _ => crate::invariant(),
    }
}

pub(crate) const fn send_control_contract_error<const FROM: u8, const TO: u8, M>()
-> Option<MessageControlContractError>
where
    M: Message,
{
    if let Some(error) = message_control_contract_error::<M>() {
        return Some(error);
    }
    if !<M as MessageRuntime>::CONTROL_PAYLOAD {
        return None;
    }
    let Some(spec) = StaticControlDesc::from_runtime_tuple(<M as MessageRuntime>::CONTROL) else {
        return Some(MessageControlContractError::MissingDescriptor);
    };
    let is_self_send = FROM == TO;
    match spec.path() {
        ControlPath::Local => {
            if is_self_send {
                None
            } else {
                Some(MessageControlContractError::ControlPathMismatch)
            }
        }
        ControlPath::Wire => {
            if is_self_send {
                Some(MessageControlContractError::ControlPathMismatch)
            } else {
                None
            }
        }
    }
}

/// Construct a single send step from `FROM` to `TO` carrying `M`.
///
/// Internal control descriptors are checked at this choreography boundary.
pub const fn send<const FROM: u8, const TO: u8, M>() -> Program<Send<FROM, TO, M>>
where
    M: Message,
{
    const {
        if FROM >= ROLE_DOMAIN_SIZE || TO >= ROLE_DOMAIN_SIZE {
            panic!("{}", ROLE_INDEX_ERROR);
        }
        if let Some(error) = send_control_contract_error::<FROM, TO, M>() {
            panic!("{}", error.message());
        }
    }
    Program::new()
}

/// Sequentially compose two protocol fragments.
pub const fn seq<LeftSteps, RightSteps>(
    left: Program<LeftSteps>,
    right: Program<RightSteps>,
) -> Program<Seq<LeftSteps, RightSteps>> {
    let _ = (left, right);
    Program::new()
}

/// Construct a binary route.
///
/// The controller is derived from the sender of the first visible action in
/// each arm. Arms whose first visible actions do not share a controller are
/// rejected during projection.
pub const fn route<LeftSteps, RightSteps>(
    left: Program<LeftSteps>,
    right: Program<RightSteps>,
) -> Program<Route<LeftSteps, RightSteps>> {
    let _ = (left, right);
    Program::new()
}

/// Construct a binary parallel composition.
pub const fn par<LeftSteps, RightSteps>(
    left: Program<LeftSteps>,
    right: Program<RightSteps>,
) -> Program<Par<LeftSteps, RightSteps>> {
    let _ = (left, right);
    Program::new()
}

impl<LeftSteps, RightSteps> Program<Route<LeftSteps, RightSteps>> {
    /// Attach an explicit route resolver to this route site.
    ///
    /// This is for routes decided by external local state rather than by the
    /// first protocol message in each arm. The resolver is attached to the
    /// route site itself. Built-in local control heads such as loop
    /// continue/break remain ordinary choreography events when they are the
    /// branch authority.
    pub const fn resolve<const RESOLVER_ID: u16>(
        self,
    ) -> Program<Resolve<Route<LeftSteps, RightSteps>, RESOLVER_ID>> {
        if RESOLVER_ID == crate::global::ControlDesc::STATIC_POLICY_SITE {
            panic!("route resolver id u16::MAX is reserved");
        }
        let _ = self;
        Program::new()
    }
}

struct ProgramProjection<Steps>(PhantomData<Steps>);

impl<Steps> ProgramProjection<Steps>
where
    Steps: ProgramTerm,
{
    const IMAGE: crate::global::compiled::lowering::CompiledProgramImage = {
        let source_data = <Steps as ProgramTerm>::PROGRAM_SOURCE;
        let source = source_data.eff_list();
        crate::global::compiled::lowering::CompiledProgramImage::scan_const(source)
    };
}

const fn validate_choreography<Steps>()
where
    Steps: ProgramTerm,
{
    let source_data = <Steps as ProgramTerm>::PROGRAM_SOURCE;
    if let Some(error) = source_data.error() {
        panic_choreography_error(error);
    }
    let source = source_data.eff_list();
    if let Some(error) = ProgramSourceError::from_dynamic_resolver_source_status(
        source.dynamic_policy_source_status(),
    ) {
        panic_choreography_error(error);
    }
    ProgramProjection::<Steps>::IMAGE.validate_projection_program();
    if let Some(error) = crate::global::compiled::lowering::projection_error_all_roles(
        &ProgramProjection::<Steps>::IMAGE,
        source,
    ) {
        panic_choreography_error(error);
    }
}

/// Single global send witness.
pub struct Send<const FROM: u8, const TO: u8, M>(PhantomData<M>);

/// Sequential composition witness.
pub struct Seq<Left, Right>(PhantomData<(Left, Right)>);

/// Binary route witness.
pub struct Route<Left, Right>(PhantomData<(Left, Right)>);

/// Binary parallel composition witness.
pub struct Par<Left, Right>(PhantomData<(Left, Right)>);

/// Explicit route resolver witness.
pub struct Resolve<Inner, const RESOLVER_ID: u16>(PhantomData<Inner>);

pub(crate) fn project<const ROLE: u8, Steps>(
    program: &Program<Steps>,
) -> crate::global::role_program::RoleProgram<ROLE>
where
    Steps: ProgramTerm,
{
    let _ = program;
    let _ = const { validate_choreography::<Steps>() };
    let image = const {
        if ROLE >= ROLE_DOMAIN_SIZE {
            panic!("{}", ROLE_INDEX_ERROR);
        }
        match ROLE {
            0 => role_projection::role_projection_image_for::<0, Steps>(),
            1 => role_projection::role_projection_image_for::<1, Steps>(),
            2 => role_projection::role_projection_image_for::<2, Steps>(),
            3 => role_projection::role_projection_image_for::<3, Steps>(),
            4 => role_projection::role_projection_image_for::<4, Steps>(),
            5 => role_projection::role_projection_image_for::<5, Steps>(),
            6 => role_projection::role_projection_image_for::<6, Steps>(),
            7 => role_projection::role_projection_image_for::<7, Steps>(),
            8 => role_projection::role_projection_image_for::<8, Steps>(),
            9 => role_projection::role_projection_image_for::<9, Steps>(),
            10 => role_projection::role_projection_image_for::<10, Steps>(),
            11 => role_projection::role_projection_image_for::<11, Steps>(),
            12 => role_projection::role_projection_image_for::<12, Steps>(),
            13 => role_projection::role_projection_image_for::<13, Steps>(),
            14 => role_projection::role_projection_image_for::<14, Steps>(),
            15 => role_projection::role_projection_image_for::<15, Steps>(),
            16..=u8::MAX => panic!("{}", ROLE_INDEX_ERROR),
        }
    };
    crate::global::role_program::role_program_from_image(image)
}
