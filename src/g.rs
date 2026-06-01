//! Choreography language used by app authors.
//!
//! `g` is the only app-facing language layer. Build local choreography terms
//! with [`send`], [`seq`], [`route`], and [`par`], then let a protocol crate
//! project and attach them.
//!
//! ```rust,ignore
//! use hibana::g;
//!
//! let request = g::send::<0, 1, g::Msg<1, u32>, 0>();
//! let reply = g::send::<1, 0, g::Msg<2, u32>, 0>();
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

mod source;
mod terms;

use core::marker::PhantomData;

use crate::control::cap::mint::{ControlOp, ControlPath};
use crate::global::{MessageRuntime, StaticControlDesc};

pub use crate::global::Message;
pub(crate) use source::{ProgramSourceData, ProgramTerm};

pub(crate) const ROLE_DOMAIN_SIZE: u8 = 16;
const ROLE_INDEX_ERROR: &str = "role index must be < 16";

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
        panic_choreography_error(self)
    }
}

const fn panic_choreography_error(error: ProgramSourceError) -> ! {
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

#[derive(Clone, Copy)]
#[repr(u8)]
pub(crate) enum MessageControlContractError {
    MissingDescriptor,
    DescriptorTagReserved,
    RouteScope,
    RoutePath,
    LoopScope,
    LoopPath,
    UnitLocal,
    LocalCrossRole,
    WireSelfSend,
    UnknownPayloadKind,
}

impl MessageControlContractError {
    pub(crate) const fn message(self) -> &'static str {
        match self {
            Self::MissingDescriptor => "control message missing descriptor",
            Self::DescriptorTagReserved => "control descriptor tag 0 is reserved",
            Self::RouteScope => "route-decision control messages require route scope",
            Self::RoutePath => "route-decision control messages require local path",
            Self::LoopScope => "loop control messages require loop scope",
            Self::LoopPath => "loop control messages require local path",
            Self::UnitLocal => "unit control payloads require local endpoint-owned controls",
            Self::LocalCrossRole => "local control messages require self-send",
            Self::WireSelfSend => "wire control messages require cross-role send",
            Self::UnknownPayloadKind => "unknown control payload kind",
        }
    }
}

const fn control_descriptor_contract_error(
    spec: StaticControlDesc,
) -> Option<MessageControlContractError> {
    if spec.resource_tag() == 0 {
        return Some(MessageControlContractError::DescriptorTagReserved);
    }
    match spec.op() {
        ControlOp::RouteDecision => {
            if !matches!(
                spec.scope_kind(),
                crate::global::const_dsl::ControlScopeKind::Route
            ) {
                return Some(MessageControlContractError::RouteScope);
            }
            if !matches!(spec.path(), ControlPath::Local) {
                return Some(MessageControlContractError::RoutePath);
            }
        }
        ControlOp::LoopContinue | ControlOp::LoopBreak => {
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
        _ => {}
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
        return Some(MessageControlContractError::UnitLocal);
    }
    None
}

pub(crate) const fn role_pair_contract_error<const FROM: u8, const TO: u8>()
-> Option<&'static str> {
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
    let Some(spec) = <M as MessageRuntime>::CONTROL else {
        return Some(MessageControlContractError::MissingDescriptor);
    };
    match <M as MessageRuntime>::CONTROL_PAYLOAD_KIND {
        1 => unit_control_payload_contract_error(spec),
        2 => control_descriptor_contract_error(spec),
        _ => Some(MessageControlContractError::UnknownPayloadKind),
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
    let Some(spec) = <M as MessageRuntime>::CONTROL else {
        return Some(MessageControlContractError::MissingDescriptor);
    };
    let is_self_send = FROM == TO;
    match spec.path() {
        ControlPath::Local if !is_self_send => Some(MessageControlContractError::LocalCrossRole),
        ControlPath::Wire if is_self_send => Some(MessageControlContractError::WireSelfSend),
        _ => None,
    }
}

/// Construct a single send step from `FROM` to `TO` carrying `M` on `LANE`.
///
/// Lanes distinguish independent conversations between the same roles. A
/// control message is checked at this choreography boundary: endpoint-owned
/// local controls are self-sends, explicit wire controls cross roles.
pub const fn send<const FROM: u8, const TO: u8, M, const LANE: u8>()
-> Program<Send<FROM, TO, M, LANE>>
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
/// The controller is derived from the first self-send control point in each arm.
/// Both arms must begin with the same controller self-send.
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

struct ProgramProjection<Steps>(PhantomData<Steps>);

impl<Steps> ProgramProjection<Steps>
where
    Steps: ProgramTerm,
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

const fn validate_choreography<Steps>()
where
    Steps: ProgramTerm,
{
    let source_data = <Steps as ProgramTerm>::PROGRAM_SOURCE;
    if let Some(error) = source_data.error() {
        panic_choreography_error(error);
    }
    let source = source_data.eff_list();
    if let Some(error) =
        ProgramSourceError::from_policy_head_status(source.dynamic_policy_source_status())
    {
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

impl<Steps> Program<Steps> {
    #[inline(always)]
    const fn compiled_program_image()
    -> &'static crate::global::compiled::lowering::CompiledProgramImage
    where
        Steps: ProgramTerm,
    {
        &ProgramProjection::<Steps>::IMAGE
    }
}

/// Single global send witness.
pub struct Send<const FROM: u8, const TO: u8, M, const LANE: u8 = 0>(PhantomData<M>);

/// Sequential composition witness.
pub struct Seq<Left, Right>(PhantomData<(Left, Right)>);

/// Binary route witness.
pub struct Route<Left, Right>(PhantomData<(Left, Right)>);

/// Binary parallel composition witness.
pub struct Par<Left, Right>(PhantomData<(Left, Right)>);

/// Dynamic-policy annotation witness.
pub struct Policy<Inner, const POLICY_ID: u16>(PhantomData<Inner>);

struct RoleProjection<const ROLE: u8, Steps>(PhantomData<Steps>);

impl<const ROLE: u8, Steps> RoleProjection<ROLE, Steps>
where
    Steps: ProgramTerm,
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
const fn role_projection_image_for<const ROLE: u8, Steps>()
-> &'static crate::global::compiled::images::CompiledRoleImage
where
    Steps: ProgramTerm,
{
    &RoleProjection::<ROLE, Steps>::IMAGE
}

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
            0 => role_projection_image_for::<0, Steps>(),
            1 => role_projection_image_for::<1, Steps>(),
            2 => role_projection_image_for::<2, Steps>(),
            3 => role_projection_image_for::<3, Steps>(),
            4 => role_projection_image_for::<4, Steps>(),
            5 => role_projection_image_for::<5, Steps>(),
            6 => role_projection_image_for::<6, Steps>(),
            7 => role_projection_image_for::<7, Steps>(),
            8 => role_projection_image_for::<8, Steps>(),
            9 => role_projection_image_for::<9, Steps>(),
            10 => role_projection_image_for::<10, Steps>(),
            11 => role_projection_image_for::<11, Steps>(),
            12 => role_projection_image_for::<12, Steps>(),
            13 => role_projection_image_for::<13, Steps>(),
            14 => role_projection_image_for::<14, Steps>(),
            15 => role_projection_image_for::<15, Steps>(),
            _ => panic!("{}", ROLE_INDEX_ERROR),
        }
    };
    crate::global::role_program::role_program_from_image(image)
}
