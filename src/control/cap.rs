//! Capability subsystem root.
//!
//! This module groups the capability components while keeping mint owners
//! explicit in downstream code.

/// Private codecs for atomic control handles.
pub(crate) mod atomic_codecs;
/// Capability minting.
pub(crate) mod mint;
/// Resource kind definitions.
pub mod resource_kinds;
/// Typed token wrappers.
pub(crate) mod typed_tokens;
