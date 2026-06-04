//! Observability surface exposing canonical observe modules.
//!
//! The no_std tap ring lives in `observe::core`. Tap event identifiers are
//! generated at build time and consumed internally by the canonical observe
//! owners.

/// Core tap ring and trace storage.
pub(crate) mod core;

/// Canonical tap event and semantic Evidence decode.
pub(crate) mod event;

/// Tap event identifiers.
pub(crate) mod ids;

/// Tap event builders.
pub(crate) mod events;

/// Scope trace helpers.
pub(crate) mod scope;

#[cfg(all(test, hibana_repo_tests))]
#[path = "observe/tests/normalise.rs"]
mod normalise;
