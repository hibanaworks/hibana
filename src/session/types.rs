//! Session witness identifiers.
//!
//! Marker traits in this module are crate-private witness vocabulary. They do
//! not prove an invariant by themselves; rendezvous owners issue typestate values
//! only after the corresponding lane, generation, or reservation check has
//! already succeeded. Public items here are compact wire/runtime identifiers.

use core::num::NonZeroU16;

/// Lane identifier.
///
/// Lanes are wire-visible `u8` values. Construction rejects values outside the
/// wire domain so resident transport/session handles cannot alias lanes by
/// truncation.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Lane(u8);

impl Lane {
    /// Create a new lane identifier.
    pub(crate) const fn new(id: u32) -> Self {
        assert!(id <= u8::MAX as u32, "lane id must be <= 255");
        Self(id as u8)
    }

    /// Get the raw lane identifier.
    pub(crate) const fn raw(self) -> u32 {
        self.0 as u32
    }

    /// Convert to wire format.
    pub(crate) const fn as_wire(self) -> u8 {
        self.0
    }
}

/// Session identifier (newtype for type safety).
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SessionId(pub u32);

impl SessionId {
    /// Create a new session identifier.
    pub const fn new(id: u32) -> Self {
        Self(id)
    }

    /// Get the raw session identifier.
    pub const fn raw(self) -> u32 {
        self.0
    }
}

/// Rendezvous identifier (newtype for type safety).
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RendezvousId(NonZeroU16);

impl RendezvousId {
    /// Create a new rendezvous identifier inside the registered-runtime owner.
    pub(crate) const fn new(id: u16) -> Self {
        match NonZeroU16::new(id) {
            Some(id) => Self(id),
            None => panic!("rendezvous id must be non-zero"),
        }
    }

    /// Get the raw rendezvous identifier.
    pub const fn raw(self) -> u16 {
        self.0.get()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[should_panic(expected = "rendezvous id must be non-zero")]
    fn rendezvous_id_zero_is_rejected() {
        let _ = RendezvousId::new(0);
    }
}
