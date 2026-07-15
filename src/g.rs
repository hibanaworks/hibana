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

mod role_projection;

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

pub(crate) const fn panic_choreography_error(error: ProgramSourceError) -> ! {
    match error {
        ProgramSourceError::RouteControllerMismatch => {
            panic!("route arms use different first visible controllers")
        }
        ProgramSourceError::ReceiveLaneCausalityConflict => {
            panic!("receive lane sender change requires a causal handoff or exclusive route arms")
        }
        ProgramSourceError::ParallelAmbiguousEndpointSelector => {
            panic!("parallel endpoint operations must be unambiguous")
        }
        ProgramSourceError::ReentryAmbiguousEndpointSelector => {
            panic!("rolled reentry endpoint operations must be unambiguous")
        }
        ProgramSourceError::ProjectionRouteUnprojectable => panic!(concat!(
            "Route unprojectable for this role: invalid, ambiguous endpoint operation, ",
            "or ambiguous first-visible endpoint operation",
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
pub const fn seq<LeftSteps, RightSteps>(
    left: Program<LeftSteps>,
    right: Program<RightSteps>,
) -> Program<Seq<LeftSteps, RightSteps>> {
    let _ = (left, right);
    Program::new()
}

/// Construct a binary route.
///
/// Intrinsic routes derive branch authority from first-visible endpoint
/// evidence; their arms must agree on the first-visible controller. Resolved
/// routes use [`ResolverRef::decide`](crate::runtime::resolver::ResolverRef::decide) as
/// branch authority instead.
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
    /// route site itself.
    pub const fn resolve<const RESOLVER_ID: u16>(
        self,
    ) -> Program<Resolve<Route<LeftSteps, RightSteps>, RESOLVER_ID>> {
        if RESOLVER_ID == u16::MAX {
            panic!("route resolver id must be < u16::MAX");
        }
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

    const VALIDATION: () = {
        let source = Self::SOURCE_EFF_LIST;
        Self::IMAGE.validate_projection_program();
        if let Some(error) =
            crate::global::compiled::lowering::projection_error_all_roles(&Self::IMAGE, source)
        {
            panic_choreography_error(error);
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
