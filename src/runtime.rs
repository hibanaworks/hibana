//! Storage, projection, codecs and transport integration.
//!
//! Use [`project`](crate::runtime::program::project) to derive a role's local protocol.
//! Initialize [`SessionKitStorage`](crate::runtime::SessionKitStorage), pass your
//! byte slab and transport to [`rendezvous`](crate::runtime::SessionKit::rendezvous),
//! then attach the role with [`enter`](crate::runtime::RendezvousKit::enter).
//! The returned [`crate::Endpoint`] borrows that runtime's storage.
//!
//! - [`program`](crate::runtime::program) contains projection and local protocol data.
//! - [`wire`](crate::runtime::wire) contains payload encoders, validators and decoders.
//! - [`transport`](crate::runtime::transport) defines buffers, frames and asynchronous I/O.
//! - [`resolver`](crate::runtime::resolver) registers application decisions for protocol choices.
//!
//! Attach and resolver operations return their own error types. Transport
//! deadlines are reported through [`TransportError`](crate::runtime::transport::TransportError). Restarting
//! after a transport failure requires a fresh run whose transport cannot expose
//! frames from the previous run.

pub use crate::session::cluster::error::AttachError;

mod buckets;
mod session_kit;
#[cfg(all(test, hibana_repo_tests))]
mod tests;

pub use buckets::*;
pub use session_kit::{RendezvousKit, SessionKit, SessionKitStorage};
