//! Capability subsystem hub.
//!
//! This module wires together the CapMint 2.0 components while preserving the
//! const-first, macro-free design that underpins the capability pipeline.

/// Capability minting.
pub mod mint;
/// Capability payload helpers.
pub mod payload;
/// Resource kind definitions.
pub mod resource_kinds;
/// Typed token wrappers.
pub mod typed_tokens;

pub use mint::*;
pub use payload::*;
pub use resource_kinds::*;
pub use typed_tokens::*;

use crate::control::types::RendezvousId;

/// Common behavior for control handle types.
///
/// This trait enables Handle-Driven Design where delegation link tracking
/// is determined by the handle's data structure rather than the Kind.
/// Standard handle types like `(u32, u16)` get empty implementations,
/// while handles with cross-rendezvous references (like `SpliceHandle`)
/// override `visit_delegation_links`.
pub trait ControlHandle: Copy + Send + Sync + 'static {
    /// Enumerate rendezvous IDs referenced by this handle.
    ///
    /// Default implementation: no delegation links.
    #[inline]
    fn visit_delegation_links(&self, _f: &mut dyn FnMut(RendezvousId)) {}
}

// Standard handle implementations (no delegation links)
impl ControlHandle for () {}
impl ControlHandle for u8 {}
impl ControlHandle for (u32, u16) {}
impl ControlHandle for (u32, u32) {}
impl ControlHandle for (u8, u64) {}
