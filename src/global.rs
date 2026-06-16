//! Global session type DSL (iso-recursive).
//!
//! This module exposes the primitives needed to assemble global choreographies
//! as local choreography witnesses and project them to role-local views.

pub(crate) use types::ROLE_DOMAIN_SIZE;

/// Crate-private lowering owners for unified compilation.
pub(crate) mod compiled;
/// Const-evaluated DSL and effect list plumbing.
pub(crate) mod const_dsl;
/// Descriptor-backed local affine event program rows.
pub(crate) mod event_program;
mod message;
/// Program combinators and route builders.
pub(crate) mod program;
pub use message::Message;
/// Role-local program projection and metadata.
pub(crate) mod role_program;
pub(crate) use role_program::RoleProgramView;
#[cfg(all(test, hibana_repo_tests))]
mod event_program_cursor_tests;
#[cfg(all(test, hibana_repo_tests))]
mod event_program_tests;
/// Type-level step combinators.
pub(crate) mod steps;
/// Role-domain constants consumed by lowering/runtime internals.
mod types;
/// Typestate graph and cursor infrastructure.
pub(crate) mod typestate;

#[cfg(all(test, hibana_repo_tests))]
mod tests;
