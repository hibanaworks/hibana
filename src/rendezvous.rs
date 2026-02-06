//! Rendezvous state machine for evaluating `control::CpEffect`.
//!
//! This module is a low-level building block used by the control plane and
//! runtime. Prefer the higher-level APIs in `control` and `runtime` unless you
//! need direct access to rendezvous tables or ports.

mod association;
mod capability;
mod core;
mod error;
mod port;
mod slots;
mod splice;
mod tables;
mod types;

pub use types::*;

#[allow(unused_imports)]
pub use error::*;

pub use tables::*;

pub use capability::*;

pub use core::*;

#[allow(unused_imports)]
pub use splice::*;

// Re-export port types

// Re-export slot arena types
pub use slots::*;
