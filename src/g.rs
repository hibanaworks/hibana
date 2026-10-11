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
//! Labels identify choreography messages. Intrinsic routes derive branch
//! authority from first-visible endpoint evidence; resolved routes derive it
//! from [`ResolverRef::decide`](crate::runtime::resolver::ResolverRef::decide).
//! Labels do not encode transport demux or hidden runtime semantics.
//!
//! Dynamic branch resolver is supplied by runtime resolvers. Runtime hints or
//! payload contents do not create route authority by themselves.

mod source;
mod terms;

use core::marker::PhantomData;

pub use crate::global::Message;
pub(crate) use source::{
    ProgramShape, ProgramSourceData, ProgramSourceNode, SourceRouteResolver, checked_source_count,
};

mod diagnostic;
mod diagnostic_message;
mod role_projection;
pub use diagnostic::{
    DiagnosticEvent, DiagnosticRange, ProjectionDiagnostic, ProjectionProblem, diagnose,
};

/// Canonical message descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Msg<const LOGICAL_LABEL: u8, Payload>(PhantomData<Payload>);

#[derive(Clone, Copy)]
#[repr(u8)]
pub(crate) enum ProgramSourceError {
    RouteControllerMismatch,
    ReceiveLaneCausalityConflict,
    ParallelAmbiguousEndpointSelector,
    ReentryAmbiguousEndpointSelector,
    ProjectionRouteUnprojectable,
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
    M::Payload: crate::transport::wire::WireEncode + crate::transport::wire::WirePayload,
{
    const {
        if FROM == TO
            && <M::Payload as crate::transport::wire::WirePayload>::SCHEMA_ID
                != <() as crate::transport::wire::WirePayload>::SCHEMA_ID
        {
            panic!("self-role actions require the unit payload");
        }
    }
    Program::new()
}

/// Mark this choreography fragment as a reentry scope.
///
/// A rolled fragment may be entered again after the fragment has completed.
/// The fragment's following continuation remains the natural exit path.
impl<Steps> Program<Steps> {
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

/// Construct a binary route.
///
/// Intrinsic routes derive branch authority from first-visible endpoint
/// evidence; their arms must agree on the first-visible controller. Resolved
/// routes use [`ResolverRef::decide`](crate::runtime::resolver::ResolverRef::decide) as
/// branch authority instead.
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
    /// Attach an explicit route resolver to this route site.
    ///
    /// This is for routes decided by external local state rather than by the
    /// first protocol message in each arm. The resolver is attached to the
    /// route site itself.
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
        Self::IMAGE.validate_projection_program();
        crate::global::compiled::lowering::projection_diagnostic(
            &Self::IMAGE,
            Self::SOURCE_EFF_LIST,
        )
    };

    // Separate constant queries bound compiler work per proof obligation.
    // The verdict below retains the same ordered rejection priority.
    const RECEIVE_CAUSAL: bool =
        crate::global::const_dsl::validate_receive_lane_causality(Self::SOURCE_EFF_LIST);
    const PARALLEL_SELECTORS: bool =
        crate::global::const_dsl::validate_parallel_endpoint_selectors(Self::SOURCE_EFF_LIST);
    const REENTRY_SELECTORS: bool =
        crate::global::const_dsl::validate_roll_reentry_endpoint_selectors(Self::SOURCE_EFF_LIST);
    const PASSIVE_ERROR: Option<ProgramSourceError> =
        crate::global::compiled::lowering::validate_passive_child_projection_guarantees(
            Self::SOURCE_EFF_LIST.scope_markers(),
        );
    const ROUTE_ERROR: Option<ProgramSourceError> =
        crate::global::compiled::lowering::validate_route_projection_guarantees(
            &Self::IMAGE,
            Self::SOURCE_EFF_LIST,
        );

    const VALIDATION: () = {
        let source = Self::SOURCE_EFF_LIST;
        Self::IMAGE.validate_projection_program();
        let error = if !Self::RECEIVE_CAUSAL {
            Some(ProgramSourceError::ReceiveLaneCausalityConflict)
        } else if !Self::PARALLEL_SELECTORS {
            Some(ProgramSourceError::ParallelAmbiguousEndpointSelector)
        } else if !Self::REENTRY_SELECTORS {
            Some(ProgramSourceError::ReentryAmbiguousEndpointSelector)
        } else if let Some(error) = Self::PASSIVE_ERROR {
            Some(error)
        } else {
            Self::ROUTE_ERROR
        };
        if let Some(error) = error {
            let diagnostic = match crate::global::compiled::lowering::projection_diagnostic(
                &Self::IMAGE,
                source,
            ) {
                Some(diagnostic) => diagnostic,
                None => ProjectionDiagnostic::from_error(error),
            };
            diagnostic_message::panic_diagnostic(diagnostic);
        }
    };
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

/// Reentry-scope witness.
pub struct Roll<Inner>(PhantomData<Inner>);

pub(crate) fn project<const ROLE: u8, Steps>(
    program: &Program<Steps>,
) -> crate::global::role_program::RoleProgram<ROLE>
where
    Steps: ProgramShape,
{
    let _ = program;
    let image = const {
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
    crate::global::role_program::role_program_from_image(image)
}
