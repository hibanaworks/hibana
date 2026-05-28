//! Typed program representation built from const DSL combinators.
//!
//! `Program<Steps>` is the public choreography owner consumed by projection and
//! attach paths. The raw `EffList` source and cheap composition hints stay
//! crate-private behind type-level source builders.

mod projection;
mod source;

pub use projection::{
    Projectable, ProjectionAtomSpec, ProjectionMetadataVisitor, ProjectionPolicySpec,
    ProjectionProgramFacts, ProjectionScopeSpec,
};

use core::marker::PhantomData;

use crate::global::LoopControlMeaning;
#[cfg(test)]
use crate::global::compiled::lowering::CompiledProgramImage;
use crate::global::const_dsl::ScopeId;
#[cfg(test)]
use crate::global::steps::StepNil;
use crate::global::steps::{PolicyEligible, PolicySteps, SeqSteps};

pub(crate) use source::{BuildProgramSource, validated_program_image};

#[cfg(all(test, hibana_repo_tests))]
pub(crate) use source::boundary_source_program_image;

/// A typed choreography witness.
///
/// `Program<Steps>` is a zero-sized compile-time proof carrier. It is not a
/// runtime image, not an attached endpoint, and not a transport handle.
///
/// On stable Rust, do not hoist `Program<_>` into `const` or `static` items.
/// Compose programs through a local `let` choreography term and immediately project
/// them through `project(&program)`.
#[derive(Clone, Copy)]
pub struct Program<Steps> {
    steps: PhantomData<Steps>,
}

#[cfg(test)]
impl Program<StepNil> {
    pub(crate) const fn empty() -> Self {
        Self::new()
    }
}

impl<Steps> Program<Steps> {
    #[inline(always)]
    pub(crate) const fn new() -> Self {
        Self { steps: PhantomData }
    }

    pub const fn policy<const POLICY_ID: u16>(self) -> Program<PolicySteps<Steps, POLICY_ID>>
    where
        Steps: PolicyEligible,
    {
        if POLICY_ID == crate::global::ControlDesc::STATIC_POLICY_SITE {
            panic!("dynamic policy id u16::MAX is reserved for static policy");
        }
        let _ = self;
        Program::new()
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) const fn program_image(&self) -> &'static CompiledProgramImage
    where
        Steps: BuildProgramSource,
    {
        validated_program_image::<Steps>()
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) const fn tail_is_loop_control(&self) -> bool
    where
        Steps: BuildProgramSource,
    {
        <Steps as BuildProgramSource>::SOURCE.tail_is_loop_control()
    }
}

impl<Universe, Steps> Projectable<Universe> for Program<Steps>
where
    Steps: BuildProgramSource,
{
    #[inline(always)]
    fn visit_projection_metadata<V: ProjectionMetadataVisitor>(&self, visitor: &mut V) {
        validated_program_image::<Steps>().visit_projection_metadata(visitor);
    }

    #[inline(always)]
    fn project<const ROLE: u8>(&self) -> crate::global::role_program::RoleProgram<ROLE> {
        crate::global::role_program::project_typed_program(self)
    }
}

pub const fn seq<LeftSteps, RightSteps>(
    left: Program<LeftSteps>,
    right: Program<RightSteps>,
) -> Program<SeqSteps<LeftSteps, RightSteps>> {
    let _ = (left, right);
    Program::new()
}

const fn add_scope_budget(lhs: u16, rhs: u16) -> u16 {
    let sum = lhs as u32 + rhs as u32;
    if sum > ScopeId::ORDINAL_CAPACITY as u32 {
        panic!("structured scope budget exceeded");
    }
    sum as u16
}

const fn is_binary_loop_route(
    left: Option<LoopControlMeaning>,
    right: Option<LoopControlMeaning>,
) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => left.arm() != right.arm(),
        _ => false,
    }
}

#[cfg(all(test, hibana_repo_tests))]
mod tests;
