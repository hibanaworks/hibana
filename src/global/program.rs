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
    Steps: crate::g::ProgramTerm,
{
    #[inline(always)]
    fn project<const ROLE: u8>(&self) -> crate::global::role_program::RoleProgram<ROLE> {
        crate::g::project(self)
    }
}

#[inline(always)]
pub(crate) fn project_unnamed<const ROLE: u8, P>(
    program: &P,
) -> crate::global::role_program::RoleProgram<ROLE>
where
    P: projection::seal::Sealed + ?Sized,
{
    <P as projection::seal::Sealed>::project(program)
}
