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

impl<Steps> Program<Steps> {
    #[inline(always)]
    pub(crate) const fn new() -> Self {
        Self { steps: PhantomData }
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) const fn program_image(&self) -> &'static CompiledProgramImage
    where
        Steps: BuildProgramSource,
    {
        validated_program_image::<Steps>()
    }
}

impl<Controller, const LOGICAL_LABEL: u8, Kind, const LANE: u8>
    Program<crate::g::Send<Controller, Controller, crate::g::Msg<LOGICAL_LABEL, (), Kind>, LANE>>
where
    Controller: crate::global::KnownRole + crate::global::RoleMarker,
    Kind: crate::control::cap::resource_kinds::DecisionPolicyControlKind,
{
    pub const fn policy<const POLICY_ID: u16>(
        self,
    ) -> Program<
        crate::g::Policy<
            crate::g::Send<Controller, Controller, crate::g::Msg<LOGICAL_LABEL, (), Kind>, LANE>,
            POLICY_ID,
        >,
    > {
        if POLICY_ID == crate::global::ControlDesc::STATIC_POLICY_SITE {
            panic!("policy id reserved");
        }
        let _ = self;
        Program::new()
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
) -> Program<crate::g::Seq<LeftSteps, RightSteps>> {
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
        (Some(LoopControlMeaning::Continue), Some(LoopControlMeaning::Break)) => true,
        (Some(_), Some(_)) => panic!("loop arm order"),
        (Some(_), None) | (None, Some(_)) => {
            panic!("loop arm pair")
        }
        _ => false,
    }
}
