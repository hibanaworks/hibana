//! Protocol-neutral integration surface for protocol implementors.
//!
//! App code should not use this module directly. Protocol crates use it to
//! project a choreography, allocate runtime storage, bind transport I/O, and
//! return an attached [`crate::Endpoint`].
//!
//! The canonical integration path is:
//!
//! ```text
//! g choreography
//!   -> integration::program::project(&program)
//!   -> integration::runtime::Config
//!   -> SessionKit::add_rendezvous_from_config
//!   -> SessionKit::rendezvous(...).session(...).role(...)
//!   -> role witness `.enter(...)`
//!   -> Endpoint
//! ```
//!
//! The everyday owners are:
//!
//! - [`integration::program`](crate::integration::program) for projection and
//!   role-local witnesses;
//! - [`integration::runtime`](crate::integration::runtime) for caller-provided
//!   buffers and clocks;
//! - [`integration::binding`](crate::integration::binding) for optional
//!   demux/channel evidence;
//! - [`integration::wire`](crate::integration::wire) for payload codecs;
//! - [`integration::transport`](crate::integration::transport) and
//!   [`integration::transport::Transport`] for I/O readiness;
//! - [`integration::policy`](crate::integration::policy) for explicit
//!   resolver-backed dynamic policy;
//! - [`integration::cap`](crate::integration::cap) for protocol-neutral control
//!   tokens.
//!
//! Lower-level `advanced` buckets exist only for implementors that need custom
//! demux, transport observation, or control-kind catalogues.
//!
//! Integration APIs surface attach and resolver failures as domain-specific
//! evidence. They do not add a public timeout, cancellation, restart helper, or
//! wide `HibanaError` layer; any fresh attempt is an integration decision that
//! starts a new session generation.
//!
//! # Unsafe Owner Contract
//!
//! This module owns host-facing in-place construction. Unsafe operations are
//! limited to initializing caller-provided resident storage, reborrowing that
//! initialized kit for the guard lifetime, and dropping it exactly once through
//! the storage owner.

pub use crate::control::cluster::error::AttachError;

mod buckets;
mod fluent;
mod session_kit;
#[cfg(all(test, feature = "std"))]
mod tests;

pub use buckets::*;
pub use session_kit::{RendezvousKit, ResidentSessionKit, RoleKit, SessionKit, SessionKitStorage};
