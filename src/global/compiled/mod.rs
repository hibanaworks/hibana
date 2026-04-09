//! Crate-private lowering owners for the unified compiled pipeline.
//!
//! This module is intentionally internal. It gives lowering a single ownership
//! layer without expanding the public API.

mod driver;
mod program;
mod role;
mod seal;

pub(crate) use self::{
    driver::{LoweringSummary, LoweringView, ProgramStamp},
    program::{
        CompiledProgramImage, ControlSemanticKind, ControlSemanticsTable, DynamicPolicySite,
    },
    role::CompiledRoleImage,
    seal::validate_all_roles,
};

#[cfg(test)]
pub(crate) use self::{
    program::{CompiledProgram, MAX_COMPILED_PROGRAM_RESOURCES},
    role::CompiledRole,
};
