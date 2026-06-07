//! Typed program representation built from const DSL combinators.
//!
//! `Program<Steps>` is the public choreography owner consumed by projection and
//! attach paths. The raw `EffList` source and cheap composition hints stay
//! crate-private behind type-level source builders.

mod projection;
pub(crate) mod source;

pub use projection::Projectable;

use crate::g::Program;

#[cfg(all(test, hibana_repo_tests))]
pub(crate) use source::boundary_source_program_image;

#[cfg(all(test, hibana_repo_tests))]
impl<const CONTROLLER: u8, const LOGICAL_LABEL: u8>
    Program<
        crate::g::Send<
            CONTROLLER,
            CONTROLLER,
            crate::g::ControlMsg<
                LOGICAL_LABEL,
                crate::control::cap::resource_kinds::LoopContinueKind,
            >,
        >,
    >
{
    pub(crate) const fn policy<const POLICY_ID: u16>(
        self,
    ) -> Program<
        crate::g::Policy<
            crate::g::Send<
                CONTROLLER,
                CONTROLLER,
                crate::g::ControlMsg<
                    LOGICAL_LABEL,
                    crate::control::cap::resource_kinds::LoopContinueKind,
                >,
            >,
            POLICY_ID,
        >,
    > {
        if POLICY_ID == crate::global::ControlDesc::STATIC_POLICY_SITE {
            panic!("dynamic policy id u16::MAX is reserved for static policy");
        }
        let _ = self;
        Program::new()
    }
}

#[cfg(all(test, hibana_repo_tests))]
impl<const CONTROLLER: u8, const LOGICAL_LABEL: u8>
    Program<
        crate::g::Send<
            CONTROLLER,
            CONTROLLER,
            crate::g::ControlMsg<LOGICAL_LABEL, crate::control::cap::resource_kinds::LoopBreakKind>,
        >,
    >
{
    pub(crate) const fn policy<const POLICY_ID: u16>(
        self,
    ) -> Program<
        crate::g::Policy<
            crate::g::Send<
                CONTROLLER,
                CONTROLLER,
                crate::g::ControlMsg<
                    LOGICAL_LABEL,
                    crate::control::cap::resource_kinds::LoopBreakKind,
                >,
            >,
            POLICY_ID,
        >,
    > {
        if POLICY_ID == crate::global::ControlDesc::STATIC_POLICY_SITE {
            panic!("dynamic policy id u16::MAX is reserved for static policy");
        }
        let _ = self;
        Program::new()
    }
}

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
