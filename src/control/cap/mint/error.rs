//! Capability token decode errors.

/// Descriptor, token header, or resource-owned handle-byte mismatch.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct CapError;
