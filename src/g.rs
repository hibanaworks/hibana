//! Curated choreography surface for app authors.
//!
//! Root `hibana::g` stays intentionally small:
//! - app DSL: [`ProgramSource`], [`Program`], [`freeze`], [`Msg`], [`Role`], [`send`], [`route`], [`par`], [`seq`]
//! - dynamic authority annotation: [`ProgramSource::policy`]
//! - protocol-implementor SPI: [`advanced`]

pub use crate::global::advanced;
pub use crate::global::program::{Program, ProgramSource, freeze};
pub use crate::global::{Msg, Role, par, route, send, seq};
