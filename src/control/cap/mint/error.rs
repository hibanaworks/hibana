//! Capability token decode errors.

/// Capability token decode error.
///
/// # Observability
/// `Mismatch` covers malformed descriptor bytes, unsupported descriptor fields,
/// typed-token mismatches, and resource-owned handle-byte decode failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapError {
    /// Descriptor, typed-token, or resource-owned handle-byte mismatch.
    Mismatch,
}
