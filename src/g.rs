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
//! transport demux or hidden runtime semantics.
//!
//! Dynamic branch resolver is supplied by runtime resolvers. Runtime hints or
//! payload contents do not create route authority by themselves.

mod source;
mod terms;

use core::marker::PhantomData;

pub use crate::global::Message;
pub(crate) use source::{ProgramSourceData, ProgramTerm};

pub(crate) const ROLE_DOMAIN_SIZE: u8 = 16;
const ROLE_INDEX_ERROR: &str = "role index must be < 16";

mod role_projection;

/// Canonical message descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Msg<const LOGICAL_LABEL: u8, Payload>(PhantomData<Payload>);

#[derive(Clone, Copy)]
#[repr(u8)]
pub(crate) enum ProgramSourceError {
    RouteArmHead,
    RouteDuplicateLabel,
    RouteControllerMismatch,
    RollBodyAbsent,
    ParallelArmAbsent,
    ParallelConflict,
    ResolverIdOutOfDomain,
    ResolverTargetNotRoute,
    ProjectionRouteResolverMismatch,
    ProjectionRouteResolverAbsent,
    ProjectionRouteUnprojectable,
}

impl ProgramSourceError {
    pub(crate) const fn from_dynamic_resolver_source_status(status: u8) -> Option<Self> {
        match status {
            0 => None,
            1 => Some(Self::ResolverTargetNotRoute),
            2..=u8::MAX => crate::invariant(),
        }
    }
}

pub(crate) const fn panic_choreography_error(error: ProgramSourceError) -> ! {
    match error {
        ProgramSourceError::RouteArmHead => {
            panic!("g::route arms must begin with a visible action")
        }
        ProgramSourceError::RouteDuplicateLabel => panic!("route arms reuse the same label"),
        ProgramSourceError::RouteControllerMismatch => {
            panic!("route arms use different first visible controllers")
        }
        ProgramSourceError::RollBodyAbsent => panic!("rolled body requires at least one step"),
        ProgramSourceError::ParallelArmAbsent => {
            panic!("g::par(left, right) arms require protocol steps")
        }
        ProgramSourceError::ParallelConflict => {
            panic!("parallel lanes must use disjoint (role, lane) pairs")
        }
        ProgramSourceError::ResolverIdOutOfDomain => {
            panic!("route resolver id must be < u16::MAX")
        }
        ProgramSourceError::ResolverTargetNotRoute => {
            panic!("route resolver can only be attached to a route")
        }
        ProgramSourceError::ProjectionRouteResolverMismatch => panic!("route resolver mismatch"),
        ProgramSourceError::ProjectionRouteResolverAbsent => panic!("route resolver absent"),
        ProgramSourceError::ProjectionRouteUnprojectable => panic!(concat!(
            "Route unprojectable for this role: arms not mergeable, ",
            "wire dispatch non-deterministic, ",
            "and no route resolver provided",
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

pub(crate) const fn role_pair_contract_error<const FROM: u8, const TO: u8>() -> Option<&'static str>
{
    if FROM >= ROLE_DOMAIN_SIZE || TO >= ROLE_DOMAIN_SIZE {
        return Some(ROLE_INDEX_ERROR);
    }
    None
}

/// Construct a single send step from `FROM` to `TO` carrying `M`.
pub const fn send<const FROM: u8, const TO: u8, M>() -> Program<Send<FROM, TO, M>>
where
    M: Message,
{
    const {
        if FROM >= ROLE_DOMAIN_SIZE || TO >= ROLE_DOMAIN_SIZE {
            panic!("{}", ROLE_INDEX_ERROR);
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
        source.dynamic_resolver_source_status(),
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

/// Reentry-scope witness.
pub struct Roll<Inner>(PhantomData<Inner>);

pub(crate) fn project<const ROLE: u8, Steps>(
    program: &Program<Steps>,
) -> crate::global::role_program::RoleProgram<ROLE>
where
    Steps: ProgramTerm,
{
    let _ = program;
    const { validate_choreography::<Steps>() };
    let image = const {
        if ROLE >= ROLE_DOMAIN_SIZE {
            panic!("{}", ROLE_INDEX_ERROR);
        }
        role_projection::role_projection_image_for::<ROLE, Steps>()
    };
    crate::global::role_program::role_program_from_image(image)
}
