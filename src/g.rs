//! Choreography language used by app authors.
//!
//! `g` is the only app-facing language layer. Build local choreography terms
//! with [`send`], [`seq`], [`route`], and [`par`], then let a protocol crate
//! project and attach them.
//!
//! ```rust,ignore
//! use hibana::g;
//!
//! let request = g::send::<g::Role<0>, g::Role<1>, g::Msg<1, u32>, 0>();
//! let reply = g::send::<g::Role<1>, g::Role<0>, g::Msg<2, u32>, 0>();
//! let program = g::seq(request, reply);
//! ```
//!
//! A [`Msg`] is a typed message descriptor:
//!
//! ```text
//! Msg<LOGICAL_LABEL, Payload, ControlKind = ()>
//! ```
//!
//! Labels identify choreography messages and route branches. They do not encode
//! transport demux or control semantics. Control meaning lives in descriptor
//! metadata derived from the optional `ControlKind`.
//!
//! Dynamic policy is explicit: annotate the choreography point with
//! [`Program::policy`]. Runtime hints or payload contents do not create policy
//! authority by themselves.

pub use crate::global::program::Program;
pub use crate::global::{Msg, Role, par, route, send, seq};
