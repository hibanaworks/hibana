//! Capability subsystem root.
//!
//! This module groups the capability components while keeping mint owners
//! explicit in downstream code.

/// Capability minting.
pub(crate) mod mint;
/// Resource kind definitions.
pub mod resource_kinds;
/// Typed token wrappers.
pub(crate) mod typed_tokens;

use crate::control::types::RendezvousId;

/// Common behavior for control handle types.
///
/// This trait enables Handle-Driven Design where delegation link tracking
/// is determined by the handle's data structure rather than the Kind.
/// Standard handle types like `(u32, u16)` provide explicit no-op implementations,
/// while handles with cross-rendezvous references (like `SpliceHandle`)
/// override `visit_delegation_links`.
pub trait ControlHandle: Copy + Send + Sync + 'static {
    /// Enumerate rendezvous IDs referenced by this handle.
    fn visit_delegation_links(&self, f: &mut dyn FnMut(RendezvousId));
}

// Standard handle implementations (no delegation links)
impl ControlHandle for () {
    fn visit_delegation_links(&self, _f: &mut dyn FnMut(RendezvousId)) {}
}

impl ControlHandle for u8 {
    fn visit_delegation_links(&self, _f: &mut dyn FnMut(RendezvousId)) {}
}

impl ControlHandle for (u32, u16) {
    fn visit_delegation_links(&self, _f: &mut dyn FnMut(RendezvousId)) {}
}

impl ControlHandle for (u32, u32) {
    fn visit_delegation_links(&self, _f: &mut dyn FnMut(RendezvousId)) {}
}

impl ControlHandle for (u8, u64) {
    fn visit_delegation_links(&self, _f: &mut dyn FnMut(RendezvousId)) {}
}
