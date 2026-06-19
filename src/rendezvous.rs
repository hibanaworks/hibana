//! Rendezvous session/lane owner.
//!
//! This module is a low-level building block used by the session runtime.
//! Prefer the higher-level APIs in `runtime` unless you need direct access to
//! rendezvous tables or ports.

mod association;
pub(crate) mod core;
pub(crate) mod error;
pub(crate) mod port;
mod recv_frame_receipt;
pub(crate) mod tables;
mod waiter;

pub(crate) use association::SessionFaultKind;
