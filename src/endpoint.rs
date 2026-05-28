//! Localside endpoint facade.
//!
//! An [`Endpoint`] is the app-facing affine executor for one projected role. It
//! is created by [`crate::integration::SessionKit`] and then advanced with the
//! four localside operations: [`Endpoint::flow`], [`Endpoint::recv`],
//! [`Endpoint::offer`], and [`RouteBranch::decode`].
//!
//! `flow` and `offer` are non-consuming previews. Committed progress happens
//! when a send, receive, or route decode succeeds. Committed endpoint failures
//! return [`EndpointError`] as diagnostic evidence and poison the current
//! session generation; they do not authorize hidden alternate progress.
//!
//! # Unsafe Owner Contract
//!
//! This module owns only the app-facing raw future and route-branch handles.
//! Unsafe operations dereference the carrier header installed by the rendezvous
//! endpoint owner; the endpoint borrow guarantees exclusive localside access,
//! and every raw future either completes, restores preview state, or fails fast
//! on post-ready reuse.

use crate::transport::wire::{CodecError, Payload, WirePayload};

/// Affine endpoint helpers.
pub(crate) mod affine;
mod branch;
/// Crate-private carrier owners for internal endpoint type packs.
pub(crate) mod carrier;
/// Control-plane helpers for endpoints.
pub(crate) mod control;
mod error;
/// Flow-based send API.
pub(crate) mod flow;
mod futures;
/// Internal endpoint kernel implementation.
pub(crate) mod kernel;
mod ops;
mod public_types;
#[cfg(all(test, hibana_repo_tests))]
mod tests;

pub use self::error::{EndpointError, EndpointResult};
pub(crate) use self::error::{
    EndpointOp, ErrorLocation, RecvError, RecvResult, SendError, SendResult,
};
pub use self::flow::Flow;
pub(crate) use self::futures::RecvFuture;
pub use self::public_types::{Endpoint, RouteBranch};

#[inline]
fn validate_wire_payload<P: WirePayload>(payload: Payload<'_>) -> Result<(), CodecError> {
    P::validate_payload(payload)
}

#[inline]
fn synthetic_wire_payload<P: WirePayload>(scratch: &mut [u8]) -> Result<Payload<'_>, CodecError> {
    P::synthetic_payload(scratch)
}
