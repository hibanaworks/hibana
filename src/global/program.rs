//! Typed program representation built from const DSL combinators.
//!
//! `Program<Steps>` is the public choreography owner consumed by projection and
//! attach paths. The raw `EffList` source and cheap composition hints stay
//! crate-private behind type-level source builders.

mod projection;

pub use projection::Projectable;

use crate::g::Program;

#[diagnostic::do_not_recommend]
impl<Steps> projection::seal::Sealed for Program<Steps>
where
    Steps: crate::g::ProgramShape,
{
    type Steps = Steps;
}

#[inline(always)]
pub(crate) fn bind_projection<const ROLE: u8, P>(
    _program: &P,
    (_, projected): (
        core::marker::PhantomData<P::Steps>,
        crate::global::role_program::RoleProgram<ROLE>,
    ),
) -> crate::global::role_program::RoleProgram<ROLE>
where
    P: projection::seal::Sealed,
{
    projected
}
