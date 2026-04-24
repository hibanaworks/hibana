//! Control-plane type-level invariants.
//!
//! This module defines marker traits and newtypes that encode invariants at the type level:
//! - No cross-lane aliasing
//! - At-most-once commit
//! - Strictly increasing generation
//! - Single-use shot discipline

/// Marker trait: guarantees no cross-lane aliasing.
///
/// Types implementing this trait ensure that multiple lanes cannot alias the same resource.
pub(crate) trait NoCrossLaneAliasing {}

/// Marker trait: guarantees at-most-once commit.
///
/// Types implementing this trait ensure that a transaction can be committed at most once.
pub(crate) trait AtMostOnceCommit {}

/// Type marker for strictly increasing generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct IncreasingGen;

/// One-shot type marker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct One;

/// Multi-shot type marker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Many;

/// Lane identifier (newtype for type safety).
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Lane(pub u32);

impl Lane {
    /// Create a new lane identifier.
    pub const fn new(id: u32) -> Self {
        Self(id)
    }

    /// Get the raw lane identifier.
    pub const fn raw(self) -> u32 {
        self.0
    }

    /// Convert to wire format (u8).
    ///
    /// # Note
    /// The wire format uses u8 for lane identifiers (maximum 256 lanes).
    /// Values >= 256 will be truncated.
    pub const fn as_wire(self) -> u8 {
        self.0 as u8
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct RendezvousId(pub u16);

impl RendezvousId {
    /// Create a new rendezvous identifier.
    pub const fn new(id: u16) -> Self {
        Self(id)
    }

    /// Get the raw rendezvous identifier.
    pub const fn raw(self) -> u16 {
        self.0
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
}
