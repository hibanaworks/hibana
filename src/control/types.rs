//! Control-plane type-level invariants.
//!
//! This module defines marker traits and newtypes that encode invariants at the type level:
//! - No cross-lane aliasing
//! - At-most-once commit
//! - Strictly increasing generation
//! - One-shot vs multi-shot discipline

/// Marker trait: guarantees no cross-lane aliasing.
///
/// Types implementing this trait ensure that multiple lanes cannot alias the same resource.
/// This is enforced by the `UniqueId` newtype in `assoc.rs`.
pub trait NoCrossLaneAliasing {}

/// Marker trait: guarantees at-most-once commit.
///
/// Types implementing this trait ensure that a transaction can be committed at most once.
pub trait AtMostOnceCommit {}

/// Marker trait: guarantees strictly increasing generation.
///
/// Types implementing this trait ensure that generation numbers are monotonically increasing.
pub trait StrictlyIncreasingGen {}

/// Type marker for strictly increasing generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IncreasingGen;

impl StrictlyIncreasingGen for IncreasingGen {}

/// Marker trait: one-shot discipline.
///
/// One-shot sessions/capabilities can be used exactly once.
pub trait OneShot: Sealed {}

/// Marker trait: multi-shot discipline.
///
/// Multi-shot sessions/capabilities can be used multiple times.
pub trait MultiShot: Sealed {}

/// Sealed trait to prevent external implementation of shot disciplines.
mod sealed {
    pub trait Sealed {}
}
use sealed::Sealed;

/// One-shot type marker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct One;

impl Sealed for One {}
impl OneShot for One {}

/// Multi-shot type marker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Many;

impl Sealed for Many {}
impl MultiShot for Many {}

/// Lane identifier (newtype for type safety).
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct LaneId(pub u32);

impl LaneId {
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
pub struct Gen(pub u16);

impl Gen {
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
    /// Gen is already u16, so this is an identity operation.
    /// Provided for consistency with LaneId::as_wire().
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

/// Universe identifier (newtype for type safety).
///
/// A universe is a collection of rendezvous instances that share a common namespace.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct UniverseId(pub u32);

impl UniverseId {
    /// Create a new universe identifier.
    pub const fn new(id: u32) -> Self {
        Self(id)
    }

    /// Get the raw universe identifier.
    pub const fn raw(self) -> u32 {
        self.0
    }
}

/// Domain identifier (newtype for trust boundaries).
///
/// A domain represents a trust boundary (e.g., same process, same node, same data center).
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct DomainId(pub u16);

impl DomainId {
    /// Same process (highest trust).
    pub const SAME_PROCESS: Self = Self(0);

    /// Same node (trusted).
    pub const SAME_NODE: Self = Self(1);

    /// Same data center (partially trusted).
    pub const SAME_DC: Self = Self(2);

    /// Remote (untrusted).
    pub const REMOTE: Self = Self(0xFFFF);

    /// Create a new domain identifier.
    pub const fn new(id: u16) -> Self {
        Self(id)
    }

    /// Get the raw domain identifier.
    pub const fn raw(self) -> u16 {
        self.0
    }

    /// Returns true if this domain requires MAC authentication.
    pub const fn requires_mac(self) -> bool {
        self.0 >= Self::SAME_DC.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gen_bump() {
        let generation = Gen::ZERO;
        assert_eq!(generation.bump(), Gen::new(1));
        assert_eq!(generation.bump().bump(), Gen::new(2));

        // Saturating behavior
        let max_gen = Gen::new(u16::MAX);
        assert_eq!(max_gen.bump(), max_gen);
    }

    #[test]
    fn test_domain_mac_requirement() {
        assert!(!DomainId::SAME_PROCESS.requires_mac());
        assert!(!DomainId::SAME_NODE.requires_mac());
        assert!(DomainId::SAME_DC.requires_mac());
        assert!(DomainId::REMOTE.requires_mac());
    }
}

// Note: ra types are just aliases to cp types (see ra/types.rs),
// so no From implementations are needed.
