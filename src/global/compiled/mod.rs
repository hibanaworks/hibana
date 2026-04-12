//! Crate-private lowering owners for the unified compiled pipeline.
//!
//! This module is intentionally internal. It gives lowering a single ownership
//! layer without expanding the public API.

mod driver;
mod lease;
mod program;
mod role;
mod seal;

pub(crate) use self::{
    driver::{LoweringSummary, LoweringView, ProgramStamp},
    lease::{
        LoweringLeaseMode, init_compiled_program_image_from_summary,
        init_compiled_role_image_from_summary, with_lowering_lease,
    },
    program::{
        CompiledProgramImage, ControlSemanticKind, ControlSemanticsTable, DynamicPolicySite,
    },
    role::CompiledRoleImage,
    seal::validate_all_roles,
};

#[cfg(test)]
pub(crate) use self::{
    lease::{
        init_compiled_role_image, with_compiled_program, with_compiled_programs,
        with_compiled_role_image,
        with_compiled_role_in_slot,
    },
    program::{CompiledProgram, MAX_COMPILED_PROGRAM_RESOURCES},
    role::CompiledRole,
};
