//! Rendezvous state machine for evaluating `control::cluster::effects::ControlOp`.
//!
//! This module is a low-level building block used by the control plane and
//! runtime. Prefer the higher-level APIs in `control` and `runtime` unless you
//! need direct access to rendezvous tables or ports.

mod association;
pub(crate) mod capability;
pub(crate) mod core;
pub(crate) mod error;
pub(crate) mod port;
pub(crate) mod slots;
mod splice;
pub(crate) mod tables;
