//! Capability token decode errors.

/// Descriptor, typed-token, or resource-owned handle-byte mismatch.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct CapError;
