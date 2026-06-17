//! Localside endpoint facade.
//!
//! An [`Endpoint`] is the app-facing affine executor for one projected role. It
//! is created by [`crate::runtime::SessionKit`] and then advanced with the
//! localside operations: [`Endpoint::send`], [`Endpoint::recv`],
//! [`Endpoint::offer`], [`RouteBranch::send`], and [`RouteBranch::recv`].
//!
//! `offer` is a non-consuming route preview.
//! Committed progress happens when a send, receive, or route branch first-step
//! operation succeeds.
//! Committed endpoint failures return [`EndpointError`] as diagnostic evidence
//! and poison the current session generation; they do not authorize hidden
//! alternate progress.
//! Successful sends, receives, and route branch first-step operations consume
//! progress. Dropped send/route previews restore their resident endpoint state.
//!
//! # Unsafe Owner Contract
//!
//! This module owns only the app-facing raw future and route-branch handles.
//! Unsafe operations dereference the carrier header installed by the rendezvous
//! endpoint owner; the endpoint borrow guarantees exclusive localside access,
//! and every raw future either completes, restores preview state, or fails fast
//! on post-ready reuse.

/// Affine endpoint helpers.
pub(crate) mod affine;
mod branch;
/// Crate-private carrier owners for internal endpoint type packs.
pub(crate) mod carrier;
mod error;
mod futures;
/// Endpoint kernel implementation.
pub(crate) mod kernel;
mod ops;
mod public_types;
/// Send future.
pub(crate) mod send;
/// Endpoint session binding helpers.
pub(crate) mod session;
#[cfg(all(test, hibana_repo_tests))]
mod tests;

pub use self::error::EndpointError;
pub(crate) use self::error::{EndpointOp, RecvError, RecvResult, SendError, SendResult};
pub(crate) use self::futures::RecvFuture;
pub use self::public_types::{Endpoint, RouteBranch};
