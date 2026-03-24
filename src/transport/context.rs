//! Policy input and attribute contracts.
//!
//! This module exposes a single provider contract:
//! - [`PolicySignalsProvider`] supplies slot-scoped [`PolicySignals`]
//!   (`input` + `attrs`) atomically.
//!
//! The core runtime treats context identifiers as opaque values. Protocol crates
//! define their own identifiers and semantics.

use crate::substrate::policy::epf::Slot;

const POLICY_ATTRS_CAPACITY: usize = 16;

/// Reserved core context identifiers surfaced through `ResolverContext::attr()`.
pub(crate) mod core {
    use super::ContextId;

    const CORE_CONTEXT_NAMESPACE: u16 = 0x0000;

    #[inline]
    const fn core_context_id(kind: u8) -> ContextId {
        ContextId::new(CORE_CONTEXT_NAMESPACE | kind as u16)
    }

    /// Rendezvous identifier owning the resolver invocation.
    pub const RV_ID: ContextId = core_context_id(0x01);
    /// Session identifier currently driving the resolver invocation.
    pub const SESSION_ID: ContextId = core_context_id(0x02);
    /// Logical lane attached to the current control decision.
    pub const LANE: ContextId = core_context_id(0x03);
    /// Control resource tag attached to the current descriptor.
    pub const TAG: ContextId = core_context_id(0x04);
    /// Transport latency observation in microseconds.
    pub const LATENCY_US: ContextId = core_context_id(0x10);
    /// Transport queue depth observation.
    pub const QUEUE_DEPTH: ContextId = core_context_id(0x11);
    /// Suggested pacing interval in microseconds.
    pub const PACING_INTERVAL_US: ContextId = core_context_id(0x12);
    /// Congestion mark count in the current transport snapshot.
    pub const CONGESTION_MARKS: ContextId = core_context_id(0x13);
    /// Retransmission count in the current transport snapshot.
    pub const RETRANSMISSIONS: ContextId = core_context_id(0x14);
    /// PTO count in the current transport snapshot.
    pub const PTO_COUNT: ContextId = core_context_id(0x15);
    /// Smoothed RTT estimate in microseconds.
    pub const SRTT_US: ContextId = core_context_id(0x16);
    /// Most recent acknowledged packet number.
    pub const LATEST_ACK_PN: ContextId = core_context_id(0x17);
    /// Congestion window estimate in bytes.
    pub const CONGESTION_WINDOW: ContextId = core_context_id(0x18);
    /// In-flight bytes estimate.
    pub const IN_FLIGHT_BYTES: ContextId = core_context_id(0x19);
    /// Transport algorithm tag: `1 = Cubic`, `2 = Reno`, `0x100 | x = Other(x)`.
    pub const TRANSPORT_ALGORITHM: ContextId = core_context_id(0x1A);
}

/// Opaque identifier for extension context entries.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ContextId(u16);

impl ContextId {
    /// Construct a new opaque context id.
    #[inline]
    pub const fn new(raw: u16) -> Self {
        Self(raw)
    }

    /// Return the raw identifier.
    #[inline]
    pub const fn raw(self) -> u16 {
        self.0
    }
}

/// Fixed-size context value (64-bit) for extension state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ContextValue(u64);

impl ContextValue {
    /// Sentinel value representing no data.
    pub const NONE: Self = Self(0);
    /// Explicit false flag for boolean-style attributes.
    pub const FALSE: Self = Self(0);
    /// Explicit true flag for boolean-style attributes.
    pub const TRUE: Self = Self(1);

    #[inline]
    pub const fn from_u8(v: u8) -> Self {
        Self(v as u64)
    }

    #[inline]
    pub const fn from_u16(v: u16) -> Self {
        Self(v as u64)
    }

    #[inline]
    pub const fn from_u32(v: u32) -> Self {
        Self(v as u64)
    }

    #[inline]
    pub const fn from_u64(v: u64) -> Self {
        Self(v)
    }

    #[inline]
    pub const fn from_pair(hi: u32, lo: u32) -> Self {
        Self(((hi as u64) << 32) | (lo as u64))
    }

    #[inline]
    pub const fn as_bool(self) -> bool {
        self.0 != 0
    }

    #[inline]
    pub const fn as_u8(self) -> u8 {
        self.0 as u8
    }

    #[inline]
    pub const fn as_u16(self) -> u16 {
        self.0 as u16
    }

    #[inline]
    pub const fn as_u32(self) -> u32 {
        self.0 as u32
    }

    #[inline]
    pub const fn as_u64(self) -> u64 {
        self.0
    }

    #[inline]
    pub const fn as_pair(self) -> (u32, u32) {
        ((self.0 >> 32) as u32, self.0 as u32)
    }

    #[inline]
    pub const fn raw(self) -> u64 {
        self.0
    }
}

/// Atomic policy signals provided by bindings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PolicySignals {
    pub input: [u32; 4],
    pub attrs: PolicyAttrs,
}

impl PolicySignals {
    pub const ZERO: Self = Self {
        input: [0; 4],
        attrs: PolicyAttrs::new(),
    };
}

impl Default for PolicySignals {
    fn default() -> Self {
        Self::ZERO
    }
}

/// Provider for slot-scoped policy signals.
///
/// Contract:
/// - Deterministic: for the same `slot` and logical instant, return the same value.
/// - Side-effect free: calling `signals()` must not mutate transport/binding state.
/// - Overlay precedence must be explicit and stable (e.g. shared -> local override).
pub trait PolicySignalsProvider {
    /// Return policy signals for the specified VM slot.
    fn signals(&self, slot: Slot) -> PolicySignals;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ContextEntry {
    id: ContextId,
    value: ContextValue,
}

impl ContextEntry {
    const fn empty() -> Self {
        Self {
            id: ContextId::new(0),
            value: ContextValue::NONE,
        }
    }
}

/// Fixed-size policy attribute map passed by value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PolicyAttrs {
    entries: [ContextEntry; POLICY_ATTRS_CAPACITY],
    len: u8,
}

impl Default for PolicyAttrs {
    fn default() -> Self {
        Self::new()
    }
}

impl PolicyAttrs {
    /// Maximum number of policy attributes.
    pub const CAPACITY: usize = POLICY_ATTRS_CAPACITY;

    /// Create an empty attribute map.
    #[inline]
    pub const fn new() -> Self {
        Self {
            entries: [ContextEntry::empty(); POLICY_ATTRS_CAPACITY],
            len: 0,
        }
    }

    /// Insert or overwrite an attribute.
    /// Returns `false` when capacity is exhausted.
    #[inline]
    pub fn insert(&mut self, id: ContextId, value: ContextValue) -> bool {
        let mut idx = 0usize;
        while idx < self.len as usize {
            if self.entries[idx].id == id {
                self.entries[idx].value = value;
                return true;
            }
            idx += 1;
        }
        if self.len as usize >= Self::CAPACITY {
            return false;
        }
        self.entries[self.len as usize] = ContextEntry { id, value };
        self.len += 1;
        true
    }

    /// Query an attribute by context id.
    #[inline]
    pub fn query(&self, id: ContextId) -> Option<ContextValue> {
        let mut idx = 0usize;
        while idx < self.len as usize {
            let entry = self.entries[idx];
            if entry.id == id {
                return Some(entry.value);
            }
            idx += 1;
        }
        None
    }

    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[inline]
    pub const fn len(&self) -> usize {
        self.len as usize
    }

    /// Deterministic 32-bit digest of current attributes (FNV-1a).
    #[inline]
    pub(crate) fn hash32(&self) -> u32 {
        const OFFSET: u32 = 0x811C_9DC5;
        const PRIME: u32 = 0x0100_0193;

        #[inline]
        fn mix_u8(mut hash: u32, byte: u8) -> u32 {
            hash ^= byte as u32;
            hash.wrapping_mul(PRIME)
        }

        #[inline]
        fn mix_u16(hash: u32, value: u16) -> u32 {
            let bytes = value.to_le_bytes();
            let hash = mix_u8(hash, bytes[0]);
            mix_u8(hash, bytes[1])
        }

        #[inline]
        fn mix_u64(hash: u32, value: u64) -> u32 {
            let bytes = value.to_le_bytes();
            let mut out = hash;
            let mut idx = 0usize;
            while idx < bytes.len() {
                out = mix_u8(out, bytes[idx]);
                idx += 1;
            }
            out
        }

        let mut hash = mix_u8(OFFSET, self.len);
        let mut idx = 0usize;
        while idx < self.len as usize {
            let entry = self.entries[idx];
            hash = mix_u16(hash, entry.id.raw());
            hash = mix_u64(hash, entry.value.raw());
            idx += 1;
        }
        hash
    }

    /// Copy attributes from another map (best-effort within fixed capacity).
    #[inline]
    pub fn copy_from(&mut self, src: &PolicyAttrs) {
        let mut idx = 0usize;
        while idx < src.len as usize {
            let entry = src.entries[idx];
            if !self.insert(entry.id, entry.value) {
                return;
            }
            idx += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_id_roundtrip() {
        let id = ContextId::new(0x1234);
        assert_eq!(id.raw(), 0x1234);
    }

    #[test]
    fn context_value_conversions() {
        assert!(ContextValue::TRUE.as_bool());
        assert!(!ContextValue::FALSE.as_bool());

        assert_eq!(ContextValue::from_u8(42).as_u8(), 42);
        assert_eq!(ContextValue::from_u16(1234).as_u16(), 1234);
        assert_eq!(ContextValue::from_u32(0x1234_5678).as_u32(), 0x1234_5678);
        assert_eq!(
            ContextValue::from_u64(0x1234_5678_9ABC_DEF0).as_u64(),
            0x1234_5678_9ABC_DEF0
        );

        let pair = ContextValue::from_pair(0xAABB_CCDD, 0x1122_3344);
        assert_eq!(pair.as_pair(), (0xAABB_CCDD, 0x1122_3344));
    }

    #[test]
    fn policy_attrs_insert_query_and_overwrite() {
        let id0 = ContextId::new(0x0001);
        let id1 = ContextId::new(0x0002);
        let mut attrs = PolicyAttrs::new();

        assert!(attrs.insert(id0, ContextValue::from_u8(1)));
        assert!(attrs.insert(id1, ContextValue::from_u8(2)));
        assert!(attrs.insert(id0, ContextValue::from_u8(9)));

        assert_eq!(attrs.len(), 2);
        assert_eq!(attrs.query(id0).unwrap().as_u8(), 9);
        assert_eq!(attrs.query(id1).unwrap().as_u8(), 2);
        assert!(attrs.query(ContextId::new(0xFFFF)).is_none());
    }

    #[test]
    fn policy_attrs_capacity() {
        let mut attrs = PolicyAttrs::new();
        for i in 0..PolicyAttrs::CAPACITY {
            assert!(attrs.insert(
                ContextId::new(i as u16 + 1),
                ContextValue::from_u16(i as u16),
            ));
        }
        assert!(!attrs.insert(ContextId::new(0xFFFE), ContextValue::from_u8(1)));
    }

    #[test]
    fn policy_signals_zero_defaults() {
        let signals = PolicySignals::ZERO;
        assert_eq!(signals.input, [0; 4]);
        assert!(signals.attrs.is_empty());
    }

    #[test]
    fn policy_signals_provider_uses_slot() {
        struct Provider;
        impl PolicySignalsProvider for Provider {
            fn signals(&self, slot: Slot) -> PolicySignals {
                let value = match slot {
                    Slot::Route => 1,
                    _ => 2,
                };
                let mut attrs = PolicyAttrs::new();
                let _ = attrs.insert(ContextId::new(0x0100), ContextValue::from_u8(value));
                PolicySignals {
                    input: [value as u32, 0, 0, 0],
                    attrs,
                }
            }
        }

        let route = Provider.signals(Slot::Route);
        let tx = Provider.signals(Slot::EndpointTx);
        assert_eq!(route.input[0], 1);
        assert_eq!(tx.input[0], 2);
        assert_eq!(
            route.attrs.query(ContextId::new(0x0100)).unwrap().as_u8(),
            1
        );
        assert_eq!(tx.attrs.query(ContextId::new(0x0100)).unwrap().as_u8(), 2);
    }

    #[test]
    fn policy_attrs_copy_from_merges() {
        let mut src = PolicyAttrs::new();
        assert!(src.insert(ContextId::new(1), ContextValue::from_u8(7)));
        assert!(src.insert(ContextId::new(2), ContextValue::from_u8(9)));
        let mut dst = PolicyAttrs::new();
        assert!(dst.insert(ContextId::new(2), ContextValue::from_u8(1)));
        dst.copy_from(&src);
        assert_eq!(dst.query(ContextId::new(1)).unwrap().as_u8(), 7);
        assert_eq!(dst.query(ContextId::new(2)).unwrap().as_u8(), 9);
    }

    #[test]
    fn policy_attrs_hash_changes_with_values() {
        let mut attrs_a = PolicyAttrs::new();
        let mut attrs_b = PolicyAttrs::new();
        assert!(attrs_a.insert(ContextId::new(1), ContextValue::from_u8(1)));
        assert!(attrs_b.insert(ContextId::new(1), ContextValue::from_u8(2)));
        assert_ne!(attrs_a.hash32(), attrs_b.hash32());
    }
}
