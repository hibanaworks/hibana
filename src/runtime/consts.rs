//! Static limits and universes shared across the crate.
//!
//! The values here intentionally favour predictability over configurability so
//! that they can be referenced inside `const` contexts without requiring
//! allocation or dynamic discovery.

/// Inclusive upper bound for labels supported by the default universe (`0..=255`).
///
/// Labels are choreography-local logical identifiers. Control semantics are held
/// by descriptor metadata, not by reserved numeric labels.
pub const LABEL_MAX: u8 = u8::MAX;

/// Default number of logical lanes per rendezvous.
///
/// Lanes are represented as `u8` throughout the crate (see
/// [`crate::control::types::Lane`]). Configuration surfaces use an exclusive
/// `u16` end bound so callers can express the full `0..256` lane domain.
pub const LANES_MAX: u16 = 8;

/// Exclusive upper bound for the complete wire lane domain.
pub const LANE_DOMAIN_SIZE: u16 = u8::MAX as u16 + 1;

/// Number of tap events maintained in the observation ring buffer.
pub const RING_EVENTS: usize = 128;

/// Size of each individual ring buffer (User and Infra).
pub const RING_BUFFER_SIZE: usize = RING_EVENTS / 2;

/// Trait implemented by types that declare a label universe.
pub trait LabelUniverse {
    /// Inclusive upper bound for valid label identifiers.
    const MAX_LABEL: u8;
}

/// Default label universe (complete `u8` label domain, `0..=255`).
#[derive(Clone, Copy, Debug, Default)]
pub struct DefaultLabelUniverse;
impl LabelUniverse for DefaultLabelUniverse {
    const MAX_LABEL: u8 = LABEL_MAX;
}
