//! Protocol-neutral runtime surface for protocol implementors.
//!
//! App code should not use this module directly. Protocol crates use it to
//! project a choreography, allocate runtime storage, bind transport I/O, and
//! return an attached [`crate::Endpoint`].
//!
//! The canonical runtime path is:
//!
//! ```text
//! g choreography
//!   -> runtime::program::project(&program)
//!   -> runtime::Config
//!   -> SessionKitStorage::uninit().init()
//!   -> kit.rendezvous(...)
//!   -> registered rendezvous .session(...).role(...)
//!   -> role witness `.enter()`
//!   -> Endpoint
//! ```
//!
//! The everyday owners are:
//!
//! - [`runtime::program`](crate::runtime::program) for projection and
//!   role-local witnesses;
//! - [`runtime::Config`](crate::runtime::Config) and
//!   [`runtime::CounterClock`](crate::runtime::CounterClock) for
//!   caller-provided buffers and clocks;
//! - [`runtime::wire`](crate::runtime::wire) for payload codecs;
//! - [`runtime::transport`](crate::runtime::transport) and
//!   [`runtime::transport::Transport`] for I/O readiness and ingress
//!   demux evidence;
//! - [`runtime::resolver`](crate::runtime::resolver) for explicit
//!   route resolver sites.
//!
//! Transport observation and resolver authority stay under their owning
//! canonical buckets; the runtime surface does not expose parallel mirrors.
//!
//! Runtime APIs surface attach and resolver failures as domain-specific
//! evidence. They do not add a public timeout, cancellation, restart helper, or
//! wide `HibanaError` layer; any fresh attempt is a runtime decision that
//! starts a new session generation.
//!
//! # Unsafe Owner Contract
//!
//! This module owns host-facing in-place construction. Unsafe operations are
//! limited to initializing caller-provided storage, reborrowing that
//! initialized kit for the guard lifetime, and dropping it exactly once through
//! the storage owner.

pub use crate::session::cluster::error::AttachError;

mod buckets;
mod fluent;
mod session_kit;
#[cfg(all(test, hibana_repo_tests, feature = "std"))]
mod tests;

pub use buckets::*;
pub use session_kit::{RendezvousKit, RoleKit, SessionKit, SessionKitStorage};
