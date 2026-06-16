//! Compile-time limits shared across the crate.
//!
//! The values here intentionally favour predictability over configurability so
//! that they can be referenced inside `const` contexts without requiring
//! allocation or dynamic discovery.

/// Exclusive upper bound for the complete wire lane domain.
pub const LANE_DOMAIN_SIZE: u16 = u8::MAX as u16 + 1;

/// Number of tap events retained in the runtime evidence ring.
pub const TAP_EVENTS: usize = 32;
