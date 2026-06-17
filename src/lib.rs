#![no_std]
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(private_bounds)]
#![deny(private_interfaces)]
#![deny(rustdoc::broken_intra_doc_links)]
#![deny(rustdoc::private_intra_doc_links)]
#![doc(html_no_source)]
#![recursion_limit = "256"]

//! Hibana is a Rust 2024 `no_std` / no-alloc-oriented runtime for affine
//! multiparty session types.
//!
//! The crate intentionally has two faces:
//!
//! - app authors use [`g`] and [`Endpoint`];
//! - protocol implementors use [`runtime`] and [`runtime::program`].
//!
//! Everything starts from one global choreography and ends in a small localside
//! endpoint:
//!
//! ```text
//! g choreography -> project role program -> attach endpoint -> drive localside
//! ```
//!
//! ## App path
//!
//! Application code writes choreography with [`g`] and drives an endpoint that a
//! protocol crate has already attached.
//!
//! ```rust,ignore
//! use hibana::g;
//!
//! let app = g::seq(
//!     g::send::<0, 1, g::Msg<1, u32>>(),
//!     g::send::<1, 0, g::Msg<2, u32>>(),
//! );
//!
//! endpoint.send::<g::Msg<1, u32>>(&7).await?;
//! let reply = endpoint.recv::<g::Msg<2, u32>>().await?;
//! ```
//!
//! The localside API is deliberately small:
//!
//! - [`Endpoint::send`] sends the next deterministic message;
//! - [`Endpoint::recv`] receives a deterministic message;
//! - [`Endpoint::offer`] observes a route branch;
//! - [`RouteBranch::label`] reports the selected choreography label;
//! - [`RouteBranch::recv`] receives the first payload in a selected receive arm;
//! - [`RouteBranch::send`] sends the first payload in a selected send arm.
//!
//! A route branch whose selected arm begins with a send is handled by
//! [`RouteBranch::send`]. Dropping the returned future restores the branch
//! preview before any progress commits.
//! Successful sends, receives, and route branch first-step operations consume
//! progress.
//!
//! ```rust,ignore
//! let branch = endpoint.offer().await?;
//! match branch.label() {
//!     10 => {
//!         let value = branch.recv::<g::Msg<10, [u8; 4]>>().await?;
//!     }
//!     11 => {
//!         branch.send::<g::Msg<11, ()>>(&()).await?;
//!     }
//!     label => panic!("unexpected route label {label}"),
//! }
//! ```
//!
//! ## Protocol path
//!
//! Protocol crates compose prefixes around an app choreography, project a
//! role-local witness, bind transport state, and return an attached endpoint.
//!
//! ```rust,ignore
//! use hibana::{g, runtime};
//! use hibana::runtime::program::{RoleProgram, project};
//!
//! let program = g::seq(transport_prefix, app);
//! let role0: RoleProgram<0> = project(&program);
//!
//! let mut slab = [0u8; 4096];
//! let mut kit_storage = runtime::SessionKitStorage::<MyTransport>::uninit();
//! let kit = kit_storage.init();
//! let rv = kit.rendezvous(&mut slab, transport)?;
//! let endpoint = rv.enter(sid, &role0)?;
//! ```
//!
//! Runtime capacities are derived from Hibana's wire/domain limits and
//! projected descriptors, not chosen by callers.
//! Hidden timeout fuses are not protocol API or attach config.
//! Protocol-invisible carrier watchdogs live inside the transport implementation:
//! terminal I/O waits are reported as [`runtime::transport::TransportError`]
//! from `poll_send` or `poll_recv`, not as Hibana timeout branches.
//!
//! [`runtime::transport::Transport`] owns I/O readiness, wire buffers, and
//! ingress demux evidence. [`runtime::resolver`] owns dynamic resolver input.
//! None of those layers become app concepts.
//!
//! ## Payloads, receive evidence, and resolvers
//!
//! Payload types implement [`runtime::wire::WireEncode`] for sends and
//! [`runtime::wire::WirePayload`] for receives. Decoded values may borrow from
//! the received frame. Built-in exact codecs cover `()`, integers, `bool`,
//! byte slices, and fixed byte arrays.
//!
//! Branch choice is either an in-band protocol message, a descriptor-checked
//! received frame, or an explicit resolver decision. Transport evidence is
//! descriptor evidence only; it is not route authority and it does not create a
//! public branch-authority catalogue.
//!
//! ## Guarantees
//!
//! Hibana keeps the public API small because the projection boundary carries the
//! proof work:
//!
//! - route shape, duplicate branch labels, and controller mismatch are rejected
//!   before runtime;
//! - parallel composition rejects empty arms and overlapping `(role, lane)`
//!   ownership;
//! - labels are choreography identities, while transport frame labels are
//!   descriptor facts;
//! - endpoint progress is affine: successful sends, receives, and route branch
//!   first-step operations commit progress, while dropped previews restore the
//!   endpoint;
//! - `EndpointError` fails closed, carries compact endpoint operation evidence,
//!   and never authorizes hidden progress.
//!
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

/// Rendezvous owner for local session, lane, and route state.
///
/// Application code uses [`Endpoint`] for choreography execution and
/// [`runtime::SessionKit`] for runtime coordination. This module stays internal;
/// tests reach it through crate-private coverage, not through a third public
/// face.
mod rendezvous;

// ============================================================================
// Re-exports (curated public API)
// ============================================================================

// Endpoint facade
pub use endpoint::{Endpoint, EndpointError, RouteBranch};
