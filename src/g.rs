//! Curated choreography surface for app authors.
//!
//! Root `hibana::g` stays intentionally small:
//! - app DSL: [`Program`], [`Msg`], [`Role`], [`send`], [`route`], [`par`], [`seq`]
//! - dynamic authority annotation: [`Program::policy`]

pub use crate::global::program::Program;
pub use crate::global::{Msg, Role, par, route, send, seq};
