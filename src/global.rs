//! Global session type DSL (iso-recursive).
//!
//! This module exposes the primitives needed to assemble global choreographies
//! as local choreography witnesses and project them to role-local views.

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
pub(crate) use message::payload_schema;
/// Role-local program projection and metadata.
pub(crate) mod role_program;
pub(crate) use role_program::RoleProgramView;
#[cfg(all(test, hibana_repo_tests))]
mod event_program_cursor_tests;
#[cfg(all(test, hibana_repo_tests))]
mod event_program_tests;
/// Typestate graph and cursor infrastructure.
pub(crate) mod typestate;

#[cfg(all(test, hibana_repo_tests))]
mod tests;
