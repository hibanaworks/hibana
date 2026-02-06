//! FFI boundary types for control-plane.
//!
//! This module defines the **single** FFI boundary for hibana's control-plane.
//! All external interfaces (unikernel, hypervisor, user-space) must go through
//! the C ABI types defined here.
//!
//! ## Design Principles
//!
//! 1. **Single Entry Point**: All FFI types are defined in this module
//! 2. **repr(transparent)**: All newtypes use `repr(transparent)` for zero-cost abstraction
//! 3. **No Padding**: All structs use `repr(C)` to ensure stable layout
//! 4. **Versioning**: All protocol messages include a version field

use crate::control::types::{DomainId, Gen, LaneId, RendezvousId, UniverseId};

/// FFI-safe lane identifier.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FfiLaneId(pub u32);

impl From<LaneId> for FfiLaneId {
    fn from(lane: LaneId) -> Self {
        Self(lane.raw())
    }
}

impl From<FfiLaneId> for LaneId {
    fn from(ffi: FfiLaneId) -> Self {
        LaneId::new(ffi.0)
    }
}

/// FFI-safe generation number.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FfiGen(pub u16);

impl From<Gen> for FfiGen {
    fn from(generation: Gen) -> Self {
        Self(generation.raw())
    }
}

impl From<FfiGen> for Gen {
    fn from(ffi: FfiGen) -> Self {
        Gen::new(ffi.0)
    }
}

/// FFI-safe rendezvous identifier.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FfiRendezvousId(pub u16);

impl From<RendezvousId> for FfiRendezvousId {
    fn from(rv_id: RendezvousId) -> Self {
        Self(rv_id.raw())
    }
}

impl From<FfiRendezvousId> for RendezvousId {
    fn from(ffi: FfiRendezvousId) -> Self {
        RendezvousId::new(ffi.0)
    }
}

/// FFI-safe universe identifier.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FfiUniverseId(pub u32);

impl From<UniverseId> for FfiUniverseId {
    fn from(universe_id: UniverseId) -> Self {
        Self(universe_id.raw())
    }
}

impl From<FfiUniverseId> for UniverseId {
    fn from(ffi: FfiUniverseId) -> Self {
        UniverseId::new(ffi.0)
    }
}

/// FFI-safe domain identifier.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FfiDomainId(pub u16);

impl From<DomainId> for FfiDomainId {
    fn from(domain_id: DomainId) -> Self {
        Self(domain_id.raw())
    }
}

impl From<FfiDomainId> for DomainId {
    fn from(ffi: FfiDomainId) -> Self {
        DomainId::new(ffi.0)
    }
}

/// Protocol version for handshake.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProtocolVersion(pub u16);

impl ProtocolVersion {
    /// Current protocol version (1.0).
    pub const V1_0: Self = Self(0x0100);

    /// Check if this version is compatible with another.
    pub fn is_compatible(self, other: Self) -> bool {
        // For now, only exact version match is supported
        self.0 == other.0
    }
}

/// Handshake message for distributed rendezvous.
///
/// This message is sent during the initial handshake to establish:
/// - Protocol version compatibility
/// - Rendezvous and universe identity
/// - Maximum label count
///
/// ## Wire Format (12 bytes)
///
/// ```text
/// +--------+--------+--------+--------+
/// | version (u16)  | rv_id (u16)     |
/// +--------+--------+--------+--------+
/// | universe_id (u32)                |
/// +--------+--------+--------+--------+
/// | max_label (u8) | padding (3 bytes)|
/// +--------+--------+--------+--------+
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hello {
    /// Protocol version (e.g., `ProtocolVersion::V1_0`)
    pub version: ProtocolVersion,

    /// Rendezvous identifier
    pub rv_id: FfiRendezvousId,

    /// Universe identifier (shared namespace)
    pub universe_id: FfiUniverseId,

    /// Maximum label value supported
    pub max_label: u8,

    /// Reserved for future use (must be zero)
    pub _reserved: [u8; 3],
}

impl Hello {
    /// Create a new handshake message.
    pub fn new(rv_id: RendezvousId, universe_id: UniverseId, max_label: u8) -> Self {
        Self {
            version: ProtocolVersion::V1_0,
            rv_id: rv_id.into(),
            universe_id: universe_id.into(),
            max_label,
            _reserved: [0; 3],
        }
    }

    /// Serialize to wire format (big-endian).
    pub fn to_bytes(self) -> [u8; 12] {
        let mut buf = [0u8; 12];
        buf[0..2].copy_from_slice(&self.version.0.to_be_bytes());
        buf[2..4].copy_from_slice(&self.rv_id.0.to_be_bytes());
        buf[4..8].copy_from_slice(&self.universe_id.0.to_be_bytes());
        buf[8] = self.max_label;
        // buf[9..12] are already zero (reserved)
        buf
    }

    /// Deserialize from wire format (big-endian).
    pub fn from_bytes(buf: &[u8; 12]) -> Self {
        Self {
            version: ProtocolVersion(u16::from_be_bytes([buf[0], buf[1]])),
            rv_id: FfiRendezvousId(u16::from_be_bytes([buf[2], buf[3]])),
            universe_id: FfiUniverseId(u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]])),
            max_label: buf[8],
            _reserved: [buf[9], buf[10], buf[11]],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hello_roundtrip() {
        let hello = Hello::new(RendezvousId::new(42), UniverseId::new(0x12345678), 255);

        let bytes = hello.to_bytes();
        let decoded = Hello::from_bytes(&bytes);

        assert_eq!(hello, decoded);
    }

    #[test]
    fn test_protocol_version_compatibility() {
        let v1 = ProtocolVersion::V1_0;
        let v2 = ProtocolVersion(0x0200);

        assert!(v1.is_compatible(v1));
        assert!(!v1.is_compatible(v2));
    }

    #[test]
    fn test_ffi_conversions() {
        let lane = LaneId::new(42);
        let ffi_lane: FfiLaneId = lane.into();
        let lane2: LaneId = ffi_lane.into();
        assert_eq!(lane, lane2);

        let generation = Gen::new(10);
        let ffi_gen: FfiGen = generation.into();
        let gen2: Gen = ffi_gen.into();
        assert_eq!(generation, gen2);
    }
}
