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
    program::{CompiledProgram, ControlSemanticKind, ControlSemanticsTable},
    role::CompiledRole,
    seal::ProjectionSeal,
};
