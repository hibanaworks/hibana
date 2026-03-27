//! Crate-private lowering owners for the unified compiled pipeline.
//!
//! This module is intentionally internal. It gives later lowering work a single
//! ownership layer without expanding the public API.

mod facts;
mod machine;

pub(crate) use self::{facts::ProgramFacts, machine::RoleMachine};
