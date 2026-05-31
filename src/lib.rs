#![cfg_attr(not(feature = "std"), no_std)]
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
//! - protocol implementors use [`integration`] and [`integration::program`].
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
//!     g::send::<0, 1, g::Msg<1, u32>, 0>(),
//!     g::send::<1, 0, g::Msg<2, u32>, 0>(),
//! );
//!
//! endpoint.flow::<g::Msg<1, u32>>()?.send(&7).await?;
//! let reply = endpoint.recv::<g::Msg<2, u32>>().await?;
//! ```
//!
//! The localside API is deliberately small:
//!
//! - [`Endpoint::flow`] previews the next send, and `.send(...)` consumes it;
//! - [`Endpoint::recv`] receives a deterministic message;
//! - [`Endpoint::offer`] observes a route branch;
//! - [`RouteBranch::label`] reports the selected choreography label;
//! - [`RouteBranch::decode`] receives the first payload in a selected receive
//!   arm.
//!
//! A route branch whose selected arm begins with a send is handled by dropping
//! the preview branch and then calling [`Endpoint::flow`] for that arm's first
//! message.
//!
//! ```rust,ignore
//! let branch = endpoint.offer().await?;
//! match branch.label() {
//!     10 => {
//!         let value = branch.decode::<g::Msg<10, [u8; 4]>>().await?;
//!     }
//!     11 => {
//!         drop(branch);
//!         endpoint.flow::<g::Msg<11, ()>>()?.send(&()).await?;
//!     }
//!     _ => unreachable!(),
//! }
//! ```
//!
//! ## Protocol path
//!
//! Protocol crates compose prefixes around an app choreography, project a
//! role-local witness, bind transport state, and return an attached endpoint.
//!
//! ```rust,ignore
//! use hibana::{g, integration};
//! use hibana::integration::program::{RoleProgram, project};
//!
//! let program = g::seq(transport_prefix, g::seq(integration_prefix, app));
//! let role0: RoleProgram<0> = project(&program);
//!
//! let mut tap_buf = [integration::runtime::TapEvent::zero(); integration::runtime::RING_EVENTS];
//! let mut slab = [0u8; 4096];
//! let clock = integration::runtime::CounterClock::new();
//! let config = integration::runtime::Config::from_resources(
//!     (&mut tap_buf, &mut slab),
//!     clock,
//! );
//! let mut kit_storage = integration::SessionKitStorage::<MyTransport>::uninit();
//! let kit = kit_storage.init();
//! let rv = kit.add_rendezvous_from_config(config, transport)?;
//! let endpoint = kit
//!     .rendezvous(rv)
//!     .session(sid)
//!     .role(&role0)
//!     .enter()?;
//! ```
//!
//! `Config` carries storage and clock only. Lane domain and endpoint slots are
//! derived from Hibana's wire/domain limits and projected descriptors, not
//! chosen by callers. Hidden timeout fuses are not protocol API or attach
//! config.
//! Protocol-invisible carrier watchdogs live inside the transport adapter:
//! terminal I/O waits are reported as [`integration::transport::TransportError`]
//! from `poll_send` or `poll_recv`, not as Hibana timeout branches.
//!
//! [`integration::transport::Transport`] owns I/O readiness and wire buffers.
//! [`integration::binding`] owns optional demux evidence. [`integration::policy`]
//! owns dynamic resolver input. None of those layers become app concepts.
//!
//! ## Payloads and control
//!
//! Payload types implement [`integration::wire::WireEncode`] for sends and
//! [`integration::wire::WirePayload`] for receives. Decoded values may borrow from
//! the received frame. Built-in exact codecs cover `()`, integers, `bool`,
//! byte slices, and fixed byte arrays.
//!
//! Control messages are ordinary [`g::Msg`] values with a control kind.
//! Public wire controls expose a `WireControlEffect`; path, shot discipline, and the
//! internal atomic operation are derived descriptor facts. Route, loop, capability,
//! and protocol-owned control messages lower into descriptor-first control facts.
//! Public code supplies semantic effects and opaque token bytes. Hibana derives
//! the control facts and executes those facts fail-closed.
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
//! - endpoint progress is affine: successful sends, receives, and route decodes
//!   commit progress, while dropped previews restore the endpoint;
//! - `EndpointError` fails closed, carries the endpoint operation callsite, and
//!   never authorizes hidden progress.
//!
//! ## Features
//!
//! The default feature set is empty. The optional `std` feature enables host
//! diagnostics and tests; it does not switch the core localside path to heap
//! ownership or change runtime semantics.

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
/// Protocol-neutral integration surface for protocol implementors.
pub mod integration;

mod control;

mod runtime;

mod transport;

mod local;

/// Session endpoints (affine-typed consuming futures)
mod endpoint;

mod observe;

mod policy_runtime;

/// Transport binding layer.
mod binding;

// ============================================================================
// Internal modules (NOT for direct user access)
// ============================================================================

mod eff;

/// Rendezvous (internal descriptor evaluator)
///
/// **INTERNAL IMPLEMENTATION - DO NOT USE DIRECTLY**
///
/// This module contains the internal implementation of the Rendezvous descriptor evaluator.
/// It evaluates descriptor-baked control facts and manages local control state.
///
/// **For application code**, use:
/// - [`Endpoint`] for localside choreography execution
/// - [`integration::SessionKit`] for Rendezvous coordination
///
/// This module stays internal; tests reach it through crate-private coverage,
/// not through a third public face.
mod rendezvous;

// ============================================================================
// Re-exports (curated public API)
// ============================================================================

// Endpoint facade
pub use endpoint::{Endpoint, EndpointError, EndpointResult, Flow, RouteBranch};
