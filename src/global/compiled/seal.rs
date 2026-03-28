use crate::{
    control::cap::mint::MintConfigMarker,
    global::{KnownRole, Role, program::Program, steps::ProjectRole, typestate::RoleTypestate},
};

use super::{LoweringSummary, ProgramStamp};

pub(crate) struct ProjectionSeal<const ROLE: u8>;

impl<const ROLE: u8> ProjectionSeal<ROLE> {
    pub(crate) const fn validate_and_stamp<Steps, Mint>(program: &Program<Steps>) -> ProgramStamp
    where
        Role<ROLE>: KnownRole,
        Steps: ProjectRole<Role<ROLE>>,
        Mint: MintConfigMarker,
    {
        let summary = LoweringSummary::scan_const(program.eff_list());
        summary.validate_projection_program();
        let typestate = RoleTypestate::<ROLE>::from_summary(&summary);
        typestate.validate_compiled_layout();
        summary.stamp()
    }
}
