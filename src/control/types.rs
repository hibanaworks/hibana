//! Control-plane witness markers and identifiers.
//!
//! Marker traits in this module are crate-private witness vocabulary. They do
//! not prove an invariant by themselves; control owners mint typestate values
//! only after the corresponding lane, generation, or reservation check has
//! already succeeded. Public items here are compact wire/runtime identifiers.

use core::num::NonZeroU16;

/// Marker trait attached to typestate witnesses after the owner has excluded
/// cross-lane aliasing for the operation being staged.
pub(crate) trait NoCrossLaneAliasing {}

/// Marker trait attached to typestate witnesses whose terminal transition is
/// consumed by value and cannot be replayed through that witness.
pub(crate) trait AtMostOnceCommit {}

/// Type marker for generation transitions validated by the rendezvous owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct IncreasingGen;

/// One-shot transaction marker consumed by terminal typestate transitions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct One;

/// Lane identifier.
///
/// Lanes are wire-visible `u8` values. Construction rejects values outside the
/// wire domain so capability and topology handles cannot silently alias lanes by
/// truncation.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Lane(u8);

impl Lane {
    /// Create a new lane identifier.
    pub const fn new(id: u32) -> Self {
        assert!(id <= u8::MAX as u32, "lane id must be <= 255");
        Self(id as u8)
    }

    /// Fallible constructor for descriptor and wire decode paths.
    pub const fn try_new(id: u32) -> Option<Self> {
        if id <= u8::MAX as u32 {
            Some(Self(id as u8))
        } else {
            None
        }
    }

    /// Get the raw lane identifier.
    pub const fn raw(self) -> u32 {
        self.0 as u32
    }

    /// Convert to wire format.
    pub const fn as_wire(self) -> u8 {
        self.0
    }
}

/// Generation number (newtype for type safety).
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Generation(pub u16);

impl Generation {
    /// Initial generation.
    pub const ZERO: Self = Self(0);

    /// Create a new generation.
    pub const fn new(value: u16) -> Self {
        Self(value)
    }

    /// Get the raw generation number.
    pub const fn raw(self) -> u16 {
        self.0
    }

    /// Convert to wire format (u16).
    ///
    /// # Note
    /// Generation is already u16, so this is an identity operation.
    /// Provided for consistency with Lane::as_wire().
    pub const fn as_wire(self) -> u16 {
        self.0
    }

    /// Increment generation (saturating).
    pub fn bump(self) -> Self {
        Self(self.0.saturating_add(1))
    }
}

/// Session identifier (newtype for type safety).
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
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
    fn test_gen_bump() {
        let generation = Generation::ZERO;
        assert_eq!(generation.bump(), Generation::new(1));
        assert_eq!(generation.bump().bump(), Generation::new(2));

        // Saturating behavior
        let max_gen = Generation::new(u16::MAX);
        assert_eq!(max_gen.bump(), max_gen);
    }

    #[test]
    #[should_panic(expected = "rendezvous id must be non-zero")]
    fn rendezvous_id_zero_is_rejected() {
        let _ = RendezvousId::new(0);
    }
}
