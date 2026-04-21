//! Static limits and universes shared across the crate.
//!
//! The values here intentionally favour predictability over configurability so
//! that they can be referenced inside `const` contexts without requiring
//! allocation or dynamic discovery.

/// Inclusive upper bound for labels supported by the default universe (`0..=127`).
///
/// `hibana` core reserves the built-in route/loop control labels below plus the
/// protocol control band `106..=127`. Sibling crates must place descriptor-first
/// control labels in that reserved band; plain payload messages may only use the
/// remaining protocol-owned labels.
pub const LABEL_MAX: u8 = 127;

// Control message labels owned by hibana core.
//
// The built-in catalogue is intentionally limited to route/loop semantics. Sibling
// crates own their own protocol control labels.
pub const LABEL_LOOP_CONTINUE: u8 = 48;
pub const LABEL_LOOP_BREAK: u8 = 49;
pub const LABEL_ROUTE_DECISION: u8 = 57;
pub(crate) const LABEL_PROTOCOL_CONTROL_MIN: u8 = 106;

/// Maximum number of logical lanes per rendezvous.
///
/// Lanes are represented as `u8` throughout the crate (see
/// [`crate::control::types::Lane`]), so this bound never exceeds `u8::MAX`. Configuration
/// surfaces such as [`crate::runtime::config::Config::with_lane_range`] refer to this
/// constant when validating caller-provided ranges.
pub const LANES_MAX: u8 = 8;

/// Number of tap events maintained in the observation ring buffer.
pub const RING_EVENTS: usize = 128;

/// Size of each individual ring buffer (User and Infra).
pub const RING_BUFFER_SIZE: usize = RING_EVENTS / 2;

/// Trait implemented by types that declare a label universe.
pub trait LabelUniverse {
    /// Inclusive upper bound for valid label identifiers.
    const MAX_LABEL: u8;
}

/// Default label universe (128 labels, 0..=127).
#[derive(Clone, Copy, Debug, Default)]
pub struct DefaultLabelUniverse;
impl LabelUniverse for DefaultLabelUniverse {
    const MAX_LABEL: u8 = LABEL_MAX;
}
