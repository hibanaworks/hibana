#![no_std]
#![deny(missing_docs)]
#![deny(clippy::wildcard_enum_match_arm)]
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(private_bounds)]
#![deny(private_interfaces)]
#![deny(rustdoc::broken_intra_doc_links)]
#![deny(rustdoc::private_intra_doc_links)]
#![doc(html_no_source)]
#![recursion_limit = "256"]

//! Choreographic programming in Rust.
//!
//! Describe communication once as a global protocol. Hibana uses multiparty
//! session type (MPST) projection to compile local protocol data. You write each
//! participant's Rust function, including its sends and receives. Hibana checks
//! those operations against the local protocol at runtime. Drive the operations
//! with an executor. Participants can run in one program or on separate machines.
//!
//! The core is `no_std`, has no normal dependencies and allocates no heap memory.
//! Runtime storage and transport buffers are supplied by your program.
//!
//! # Define a protocol
//!
//! ```
//! use hibana::{g, runtime::program::project};
//!
//! type Request = g::Msg<1, u32>;
//! type Reply = g::Msg<2, u32>;
//!
//! let global = g::seq(
//!     g::send::<0, 1, Request>(),
//!     g::send::<1, 0, Reply>(),
//! );
//! let client_program = project::<0>(&global);
//! let server_program = project::<1>(&global);
//! ```
//!
//! [`g`] provides sequential composition, choice, parallel composition and
//! repetition. Projection checks the protocol structure, ownership and branch
//! knowledge. A role's handwritten Rust code is checked when it calls its endpoint.
//!
//! # Use an endpoint
//!
//! [`Endpoint::send`] and [`Endpoint::recv`] perform the next permitted operation.
//! [`Endpoint::offer`] previews a choice without advancing the protocol. Use
//! [`RouteBranch::label`] to inspect it and [`RouteBranch::send`] or
//! [`RouteBranch::recv`] to perform its first operation.
//!
//! [`Endpoint::par`] borrows the independent fragments of a projected parallel
//! region. Poll their operations concurrently with your executor. The parent
//! endpoint is unavailable while these borrows exist.
//!
//! Ordinary payloads are borrowed for sends. An [`Endpoint`] payload is moved into
//! the send future, and its receiver gets the same logical role's continuation.
//! The sender cannot use the moved value. Dropping or failing the send does not
//! restore its ownership.
//!
//! Ownership is affine: an endpoint can be moved or dropped, but cannot be
//! copied. It can execute successive protocol steps, each at most once per
//! repetition. Remote participants observe cancellation through the transport.
//! This differs from the cancellation propagation algorithm in
//! [affine multiparty session types (AMPST)](https://arxiv.org/abs/2204.13464).
//!
//! The runnable examples show complete setup and participant code:
//!
//! - [`ping_pong`](https://github.com/hibanaworks/hibana/blob/v0.10.0/examples/ping_pong.rs)
//!   sends a number and returns its successor.
//! - [`endpoint_delegation`](https://github.com/hibanaworks/hibana/blob/v0.10.0/examples/endpoint_delegation.rs)
//!   lends an endpoint to another runtime, performs independent work in parallel,
//!   and returns the endpoint to its original holder.
//!
//! # Set up storage and transport
//!
//! [`runtime::program`] projects roles. [`runtime::SessionKitStorage`] holds the
//! runtime's storage, and [`runtime::SessionKit::rendezvous`] combines a byte slab
//! with a transport. Attach each role with [`runtime::RendezvousKit::enter`].
//! The transport implements [`runtime::transport::Transport`]. Custom message
//! formats use [`runtime::wire::WireEncode`] and [`runtime::wire::WirePayload`].
//!
//! For endpoint delegation, the resource integration supplies one
//! [`runtime::transport::EndpointBinding`]. Attach the initial holder's role
//! before attaching that resource's endpoint, in the same runtime and session.
//! The transport must keep accepted messages available across holder changes.
//!
//! # Errors and guarantees
//!
//! A successful send means that the transport accepted a frame. It does not
//! mean the peer has received it. Transport errors stop the local session run
//! and never undo a remote effect or restore transferred ownership.
//!
//! Protocol progress requires participants to follow the same global protocol,
//! invoke and poll permitted operations, and use a transport that provides peer
//! binding, FIFO delivery, replay exclusion and separation of session runs.
//! Arbitrary Rust computations and dependency cycles between separate sessions
//! are outside that guarantee.

#[cfg(test)]
extern crate self as hibana;

#[cfg(test)]
extern crate std;

#[cfg(all(test, hibana_repo_tests))]
mod test_support;

// ============================================================================
// Public modules (application-facing)
// ============================================================================

pub mod g;
/// Global-to-Local projection (MPST theory layer)
mod global;
/// Runtime surface for protocol implementors.
pub mod runtime;

mod session;

mod runtime_core;

mod transport;

mod local;

/// Session endpoints (affine-typed consuming futures)
mod endpoint;

mod observe;

// ============================================================================
// Private modules
// ============================================================================
mod eff;

#[cold]
#[inline(never)]
#[track_caller]
pub(crate) const fn invariant() -> ! {
    panic!()
}

#[inline]
#[track_caller]
pub(crate) fn invariant_some<T>(value: Option<T>) -> T {
    match value {
        Some(value) => value,
        None => invariant(),
    }
}

#[inline]
#[track_caller]
pub(crate) fn invariant_ok<T, E>(value: Result<T, E>) -> T {
    match value {
        Ok(value) => value,
        Err(_) => invariant(),
    }
}

/// Local storage and session registration.
mod rendezvous;

// ============================================================================
// Re-exports (curated public API)
// ============================================================================

// Endpoint facade
pub use endpoint::{Endpoint, EndpointError, ParallelEndpoint, RouteBranch};
