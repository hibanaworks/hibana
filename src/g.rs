//! Build a global protocol and project it for each participant.
//!
//! [`send`] describes one message, [`seq`] orders fragments, [`par`] describes
//! independent work, and [`route`] describes a choice. [`Program::roll`] makes
//! a completed fragment repeatable. A choice can use an application decision
//! registered through [`runtime::resolver`](crate::runtime::resolver).
//!
//! ```
//! use hibana::{g, runtime::program::project};
//!
//! type Request = g::Msg<1, u32>;
//! type Reply = g::Msg<2, u32>;
//! let global = g::seq(
//!     g::send::<0, 1, Request>(),
//!     g::send::<1, 0, Reply>(),
//! );
//! let client = project::<0>(&global);
//! let server = project::<1>(&global);
//! ```
//!
//! [`Msg`] associates a logical message label with its payload type. An Endpoint
//! payload transfers the right to continue that logical role. Message labels
//! are protocol identifiers. The transport's frame labels are assigned during
//! projection.

mod source;
mod terms;

use core::marker::PhantomData;

pub use crate::global::Message;
pub(crate) use source::{EndpointKey, SourcePayload};
pub(crate) use source::{
    ProgramShape, ProgramSourceData, ProgramSourceNode, SourceActorImage, checked_source_count,
};
pub(crate) use source::{SourceArm, SourceContext, SourceScope, SourceSelection};

mod diagnostic;
mod diagnostic_message;
mod role_projection;
pub use diagnostic::{
    DiagnosticEvent, DiagnosticRange, ProjectionDiagnostic, ProjectionProblem, diagnose,
};

/// A logical message label and its payload type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Msg<const LOGICAL_LABEL: u8, Payload>(PhantomData<Payload>);

#[derive(Clone, Copy)]
#[repr(u8)]
pub(crate) enum ProgramSourceError {
    RouteControllerMismatch,
    ReceiveLaneCausalityConflict,
    ParallelAmbiguousEndpointSelector,
    ReentryAmbiguousEndpointSelector,
    ReentryResourceKnowledge,
    ProjectionRouteUnprojectable,
}

/// A typed choreography term.
///
/// `Program<Steps>` is a zero-sized global protocol value. Projection validates
/// it and produces a [`RoleProgram`](crate::runtime::program::RoleProgram).
///
/// Compose protocols with local `let` bindings so Rust infers `Steps`, then use
/// [`project`](crate::runtime::program::project) for each role.
pub struct Program<Steps> {
    steps: PhantomData<Steps>,
}

impl<Steps> Copy for Program<Steps> {}
impl<Steps> Clone for Program<Steps> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<Steps> Program<Steps> {
    #[inline(always)]
    pub(crate) const fn new() -> Self {
        Self { steps: PhantomData }
    }
}

/// Construct a single send step from `FROM` to `TO` carrying `M`.
///
/// A self-role step is endpoint-local control and therefore carries only the
/// canonical zero-byte unit schema. Payload-bearing communication requires
/// distinct sender and receiver roles.
pub const fn send<const FROM: u8, const TO: u8, M>() -> Program<Send<FROM, TO, M>>
where
    M: Message,
{
    const {
        if FROM == TO
            && !matches!(
                crate::global::source_payload::<M>(),
                SourcePayload::Wire { schema: 0 }
            )
        {
            panic!("self-role actions require the unit payload");
        }
    }
    Program::new()
}

impl<Steps> Program<Steps> {
    /// Allow this fragment to run again after completion.
    ///
    /// Projection requires guarded repetition. The following sequential
    /// continuation remains available under the projected dependencies.
    pub const fn roll(self) -> Program<Roll<Steps>> {
        let _ = self;
        Program::new()
    }
}

/// Sequentially compose two protocol fragments.
pub const fn seq<
    Left: crate::global::program::Projectable,
    Right: crate::global::program::Projectable,
>(
    left: Left,
    right: Right,
) -> Program<Seq<Left::Steps, Right::Steps>> {
    let _ = (left, right);
    Program::new()
}

/// Construct a choice between two protocol fragments.
///
/// Both branches must have the same choosing role. By default, the first
/// distinguishable message identifies the branch. Use [`Program::resolve`] to
/// select it with a registered local function instead. Projection checks that
/// other participants can learn the choice before acting on it.
pub const fn route<
    Left: crate::global::program::Projectable,
    Right: crate::global::program::Projectable,
>(
    left: Left,
    right: Right,
) -> Program<Route<Left::Steps, Right::Steps>> {
    let _ = (left, right);
    Program::new()
}

/// Construct a binary parallel composition.
pub const fn par<
    Left: crate::global::program::Projectable,
    Right: crate::global::program::Projectable,
>(
    left: Left,
    right: Right,
) -> Program<Par<Left::Steps, Right::Steps>> {
    let _ = (left, right);
    Program::new()
}

impl<LeftSteps, RightSteps> Program<Route<LeftSteps, RightSteps>> {
    /// Select this choice with a registered local function, called a resolver.
    ///
    /// Register the function under `RESOLVER_ID` with
    /// [`set_resolver`](crate::runtime::RendezvousKit::set_resolver). It can use
    /// application state to choose a branch. Other participants still need
    /// protocol messages to learn that choice.
    pub const fn resolve<const RESOLVER_ID: u16>(
        self,
    ) -> Program<Resolve<Route<LeftSteps, RightSteps>, RESOLVER_ID>> {
        let _ = self;
        Program::new()
    }
}

struct ProgramProjection<Steps, const CAPACITY: usize>(PhantomData<Steps>);

impl<Steps, const CAPACITY: usize> ProgramProjection<Steps, CAPACITY>
where
    Steps: ProgramShape,
{
    const SOURCE: ProgramSourceData<CAPACITY> = ProgramSourceData::lower::<Steps>();
    pub(super) const SOURCE_EFF_LIST: &'static crate::global::const_dsl::EffList<CAPACITY> =
        Self::SOURCE.eff_list();

    const IMAGE: crate::global::compiled::lowering::CompiledProgramImage = {
        let source = Self::SOURCE_EFF_LIST;
        crate::global::compiled::lowering::CompiledProgramImage::scan_const(source)
    };

    const DIAGNOSTIC: Option<ProjectionDiagnostic> = {
        if let Some(diagnostic) = ProgramSourceData::<CAPACITY>::ownership_diagnostic::<Steps>() {
            Some(diagnostic)
        } else {
            // Defer references to the lowered constants until ownership has
            // succeeded; rustc can evaluate associated constants eagerly.
            Self::lowered_diagnostic()
        }
    };

    const fn lowered_diagnostic() -> Option<ProjectionDiagnostic> {
        Self::IMAGE.validate_projection_program();
        match Self::ERROR {
            None => None,
            Some(ProgramSourceError::RouteControllerMismatch) => {
                Some(crate::global::compiled::lowering::projection_diagnostic(
                    &Self::IMAGE,
                    Self::SOURCE_EFF_LIST,
                    ProgramSourceError::RouteControllerMismatch,
                ))
            }
            Some(error) => Some(if Steps::SOURCE_NODE.has_delegation() {
                let mut diagnostic = ProjectionDiagnostic::from_error(error);
                if matches!(error, ProgramSourceError::ReceiveLaneCausalityConflict)
                    && let Some(actors) = Self::SOURCE.actors()
                    && let Some((first, second, scope)) =
                        crate::global::const_dsl::owned_receive_lane_conflict(
                            Self::SOURCE_EFF_LIST,
                            actors,
                        )
                {
                    let earlier = Self::SOURCE_EFF_LIST.atom_at(first);
                    diagnostic.role = Some(earlier.to);
                    diagnostic.scope = scope;
                    diagnostic.first = Some(DiagnosticEvent::from_atom(first, earlier));
                    diagnostic.second = Some(DiagnosticEvent::from_atom(
                        second,
                        Self::SOURCE_EFF_LIST.atom_at(second),
                    ));
                }
                if matches!(error, ProgramSourceError::ProjectionRouteUnprojectable)
                    && let Some(actors) = Self::SOURCE.actors()
                    && let Some((scope, role, event)) =
                        actors.branch_knowledge_conflict(&Steps::SOURCE_NODE)
                {
                    diagnostic.problem = ProjectionProblem::MissingBranchKnowledge;
                    diagnostic.role = role;
                    diagnostic.arms = Some([
                        DiagnosticRange {
                            start: scope.start as usize,
                            end: scope.split as usize,
                        },
                        DiagnosticRange {
                            start: scope.split as usize,
                            end: scope.stop as usize,
                        },
                    ]);
                    let markers = Self::SOURCE_EFF_LIST.scope_markers();
                    let mut index = 0;
                    while index < markers.len() {
                        let marker = markers.at(index);
                        if marker.event.is_primary_enter()
                            && matches!(
                                marker.scope_id.kind(),
                                Some(crate::global::const_dsl::ScopeKind::Route)
                            )
                            && marker.offset() == scope.start as usize
                            && marker.segment_end() == scope.split as usize
                        {
                            diagnostic.scope = Some(marker.scope_id.local_ordinal());
                            break;
                        }
                        index += 1;
                    }
                    if let Some(event) = event {
                        let witness = Some(DiagnosticEvent::from_atom(
                            event,
                            Self::SOURCE_EFF_LIST.atom_at(event),
                        ));
                        if event < scope.split as usize {
                            diagnostic.first = witness;
                        } else {
                            diagnostic.second = witness;
                        }
                    }
                }
                diagnostic
            } else {
                crate::global::compiled::lowering::projection_diagnostic(
                    &Self::IMAGE,
                    Self::SOURCE_EFF_LIST,
                    error,
                )
            }),
        }
    }

    // Separate constant queries bound compiler work per proof obligation.
    // The verdict below retains the same ordered rejection priority.
    const RECEIVE_CAUSAL: bool = Self::SOURCE.receive_causal();
    const PARALLEL_SELECTORS: bool =
        crate::global::const_dsl::validate_parallel_endpoint_selectors(Self::SOURCE_EFF_LIST);
    const REENTRY_SELECTORS: bool =
        crate::global::const_dsl::validate_roll_reentry_endpoint_selectors(Self::SOURCE_EFF_LIST);
    const REENTRY_KNOWLEDGE: bool = match Self::SOURCE.actors() {
        Some(actors) => actors.has_reentry_knowledge(&Steps::SOURCE_NODE),
        None => true,
    };
    const PASSIVE_ERROR: Option<ProgramSourceError> =
        crate::global::compiled::lowering::validate_passive_child_projection_guarantees(
            Self::SOURCE_EFF_LIST.scope_markers(),
        );
    const ROUTE_ERROR: Option<ProgramSourceError> = if Steps::SOURCE_NODE.has_delegation() {
        if let Some(error) =
            crate::global::compiled::lowering::validate_route_control_shapes(Self::SOURCE_EFF_LIST)
        {
            Some(error)
        } else if !Self::SOURCE.actor_branch_knowledge(&Steps::SOURCE_NODE) {
            Some(ProgramSourceError::ProjectionRouteUnprojectable)
        } else {
            None
        }
    } else {
        crate::global::compiled::lowering::validate_route_projection_guarantees(
            &Self::IMAGE,
            Self::SOURCE_EFF_LIST,
        )
    };

    const ERROR: Option<ProgramSourceError> = {
        if !Self::RECEIVE_CAUSAL {
            Some(ProgramSourceError::ReceiveLaneCausalityConflict)
        } else if !Self::PARALLEL_SELECTORS {
            Some(ProgramSourceError::ParallelAmbiguousEndpointSelector)
        } else if !Self::REENTRY_SELECTORS {
            Some(ProgramSourceError::ReentryAmbiguousEndpointSelector)
        } else if !Self::REENTRY_KNOWLEDGE {
            Some(ProgramSourceError::ReentryResourceKnowledge)
        } else if let Some(error) = Self::PASSIVE_ERROR {
            Some(error)
        } else {
            Self::ROUTE_ERROR
        }
    };

    const VALIDATION: () = {
        if let Some(diagnostic) = Self::DIAGNOSTIC {
            diagnostic_message::panic_diagnostic(diagnostic);
        }
    };
}

/// A message from `FROM` to `TO`, constructed by [`send`].
pub struct Send<const FROM: u8, const TO: u8, M>(PhantomData<M>);

/// Sequential composition, constructed by [`seq`].
pub struct Seq<Left, Right>(PhantomData<(Left, Right)>);

/// A binary choice, constructed by [`route`].
pub struct Route<Left, Right>(PhantomData<(Left, Right)>);

/// Independent parallel composition, constructed by [`par`].
pub struct Par<Left, Right>(PhantomData<(Left, Right)>);

/// A choice selected by a registered application resolver.
pub struct Resolve<Inner, const RESOLVER_ID: u16>(PhantomData<Inner>);

/// A repeatable protocol fragment, constructed by [`Program::roll`].
pub struct Roll<Inner>(PhantomData<Inner>);

pub(crate) const fn project_image<const ROLE: u8, Steps>() -> (
    PhantomData<Steps>,
    crate::global::role_program::RoleProgram<ROLE>,
)
where
    Steps: ProgramShape,
{
    let image = {
        let required_rows = <Steps as ProgramShape>::SOURCE_ROW_COUNT;
        if required_rows <= 8 {
            role_projection::role_projection_image_for::<ROLE, Steps, 8>()
        } else if required_rows <= 32 {
            role_projection::role_projection_image_for::<ROLE, Steps, 32>()
        } else if required_rows <= 128 {
            role_projection::role_projection_image_for::<ROLE, Steps, 128>()
        } else if required_rows <= 512 {
            role_projection::role_projection_image_for::<ROLE, Steps, 512>()
        } else if required_rows <= 2048 {
            role_projection::role_projection_image_for::<ROLE, Steps, 2048>()
        } else if required_rows <= 8192 {
            role_projection::role_projection_image_for::<ROLE, Steps, 8192>()
        } else if required_rows <= 32768 {
            role_projection::role_projection_image_for::<ROLE, Steps, 32768>()
        } else if required_rows <= 65535 {
            role_projection::role_projection_image_for::<ROLE, Steps, 65535>()
        } else {
            panic!("choreography source exceeds compact descriptor domain")
        }
    };
    (
        PhantomData,
        crate::global::role_program::role_program_from_image(image),
    )
}
