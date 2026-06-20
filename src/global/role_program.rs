//! Role-local program representation derived from const `EffList`.
//!
//! `RoleProgram` is the typed entry point for a role projection witness.
//! Crate-private lowering facts stay behind this module and the compiled layer.

use super::compiled::lowering::RoleCompiledCounts;
use crate::global::const_dsl::{ScopeEvent, ScopeId, ScopeKind, ScopeMarker};
use core::marker::PhantomData;

mod image_impl;
mod image_types;
mod lane_set;
mod program;
#[cfg(all(test, hibana_repo_tests))]
mod tests;

pub use program::{RoleProgram, project};

pub(crate) use image_types::*;
pub(crate) use lane_set::*;
pub(crate) use program::role_program_from_image;
