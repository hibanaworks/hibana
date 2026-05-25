//! Role-local program representation derived from const `EffList`.
//!
//! `RoleProgram` is the typed entry point for a role projection witness.
//! Crate-private lowering facts stay behind this module and the compiled layer.

#![allow(unused_imports)]

use super::compiled::lowering::{CompiledProgramImage, ProgramStamp, RoleCompiledCounts};
use super::program::{BuildProgramSource, Program, validated_program_image};
use crate::global::const_dsl::{CompactScopeId, ScopeEvent, ScopeId, ScopeKind, ScopeMarker};
use core::marker::PhantomData;

mod image_impl;
mod image_types;
mod lane_set;
mod program;
#[cfg(test)]
mod tests;

pub use program::{RoleProgram, project};

pub(crate) use image_impl::*;
pub(crate) use image_types::*;
pub(crate) use lane_set::*;
pub(crate) use program::*;
