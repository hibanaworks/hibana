//! Crate-private lowering owners for the unified compiled pipeline.
//!
//! This module is intentionally internal. It keeps the public-facing law small
//! while grouping internal owners by phase: lowering validation, sealed runtime
//! images, and transient materialization glue.

#[path = "lowering/driver.rs"]
mod driver;
#[path = "images/image.rs"]
mod image;
#[path = "materialize/lease.rs"]
mod lease;
#[path = "images/program.rs"]
mod program;
#[path = "images/role.rs"]
mod role;
#[path = "lowering/seal.rs"]
mod seal;

pub(crate) use self::{
    driver::{LoweringSummary, LoweringView, ProgramStamp, RoleLoweringCounts},
    image::{ProgramImage, RoleImageSlice},
    lease::{
        LoweringLeaseMode, RoleLoweringScratch, init_compiled_program_image_from_summary,
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
        with_compiled_role_image, with_compiled_role_in_slot,
    },
    program::{CompiledProgram, MAX_COMPILED_PROGRAM_RESOURCES},
    role::CompiledRole,
};
