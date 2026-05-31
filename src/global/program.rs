//! Typed program representation built from const DSL combinators.
//!
//! `Program<Steps>` is the public choreography owner consumed by projection and
//! attach paths. The raw `EffList` source and cheap composition hints stay
//! crate-private behind type-level source builders.

mod projection;
pub(crate) mod source;

pub use projection::{
    Projectable, ProjectionAtomSpec, ProjectionMetadataVisitor, ProjectionPolicySpec,
    ProjectionProgramFacts, ProjectionScopeSpec,
};

use crate::g::Program;

#[cfg(all(test, hibana_repo_tests))]
pub(crate) use source::boundary_source_program_image;

impl<Controller, const LOGICAL_LABEL: u8, const LANE: u8>
    Program<
        crate::g::Send<
            Controller,
            Controller,
            crate::g::Msg<
                LOGICAL_LABEL,
                (),
                crate::control::cap::resource_kinds::RouteDecisionKind,
            >,
            LANE,
        >,
    >
where
    Controller: crate::global::KnownRole + crate::global::RoleMarker,
{
    pub const fn policy<const POLICY_ID: u16>(
        self,
    ) -> Program<
        crate::g::Policy<
            crate::g::Send<
                Controller,
                Controller,
                crate::g::Msg<
                    LOGICAL_LABEL,
                    (),
                    crate::control::cap::resource_kinds::RouteDecisionKind,
                >,
                LANE,
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

impl<Controller, const LOGICAL_LABEL: u8, const LANE: u8>
    Program<
        crate::g::Send<
            Controller,
            Controller,
            crate::g::Msg<LOGICAL_LABEL, (), crate::control::cap::resource_kinds::LoopContinueKind>,
            LANE,
        >,
    >
where
    Controller: crate::global::KnownRole + crate::global::RoleMarker,
{
    pub const fn policy<const POLICY_ID: u16>(
        self,
    ) -> Program<
        crate::g::Policy<
            crate::g::Send<
                Controller,
                Controller,
                crate::g::Msg<
                    LOGICAL_LABEL,
                    (),
                    crate::control::cap::resource_kinds::LoopContinueKind,
                >,
                LANE,
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

impl<Controller, const LOGICAL_LABEL: u8, const LANE: u8>
    Program<
        crate::g::Send<
            Controller,
            Controller,
            crate::g::Msg<LOGICAL_LABEL, (), crate::control::cap::resource_kinds::LoopBreakKind>,
            LANE,
        >,
    >
where
    Controller: crate::global::KnownRole + crate::global::RoleMarker,
{
    pub const fn policy<const POLICY_ID: u16>(
        self,
    ) -> Program<
        crate::g::Policy<
            crate::g::Send<
                Controller,
                Controller,
                crate::g::Msg<
                    LOGICAL_LABEL,
                    (),
                    crate::control::cap::resource_kinds::LoopBreakKind,
                >,
                LANE,
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
impl<Universe, Steps> Projectable<Universe> for Program<Steps>
where
    Steps: crate::g::ProgramTerm<Source = crate::g::ProgramSourceData>,
{
    #[inline(always)]
    fn visit_projection_metadata<V: ProjectionMetadataVisitor>(&self, visitor: &mut V) {
        Program::<Steps>::validated_program_image().visit_projection_metadata(visitor);
    }

    #[inline(always)]
    fn project<const ROLE: u8>(&self) -> crate::global::role_program::RoleProgram<ROLE> {
        crate::g::project_role(self)
    }
}

pub const fn seq<LeftSteps, RightSteps>(
    left: Program<LeftSteps>,
    right: Program<RightSteps>,
) -> Program<crate::g::Seq<LeftSteps, RightSteps>> {
    let _ = (left, right);
    Program::new()
}
