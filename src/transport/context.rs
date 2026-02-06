//! Transport context provider for protocol-specific state access.
//!
//! This module provides a protocol-agnostic trait for accessing transport-layer state
//! from resolver functions. It eliminates the need for global registries by allowing
//! transport implementations to directly expose their state through a key-value interface.
//!
//! # Design
//!
//! The `TransportContextProvider` trait uses fixed-size keys and values to maintain
//! compatibility with `no_std` / `no_alloc` environments while providing O(1) access
//! to protocol-specific state.
//!
//! # Protocol Namespacing
//!
//! Context keys are namespaced by protocol ID to prevent collisions:
//! - `0x01` - QUIC (hibana-quic)
//! - `0x02` - Raft (future)
//! - `0x03+` - User-defined protocols

/// Well-known protocol identifiers for context key namespacing.
pub mod protocol {
    /// Reserved (invalid protocol).
    pub const RESERVED: u8 = 0x00;
    /// QUIC transport protocol (RFC 9000).
    pub const QUIC: u8 = 0x01;
    /// Raft consensus protocol (reserved for future use).
    pub const RAFT: u8 = 0x02;
}

const CONTEXT_SNAPSHOT_CAPACITY: usize = 16;
const CONTEXT_PROTOCOL_SLOTS: usize = CONTEXT_SNAPSHOT_CAPACITY;
const CONTEXT_INDEX_NONE: u8 = u8::MAX;

/// Fixed-size context key for O(1) transport state lookup.
///
/// Format: `[protocol:8][kind:8]` = 16 bits total.
///
/// The `protocol` byte identifies the transport protocol (QUIC, Raft, etc.),
/// while `kind` identifies the specific state type within that protocol.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ContextKey(u16);

impl ContextKey {
    /// Create a new context key from protocol and kind identifiers.
    #[inline]
    pub const fn new(protocol: u8, kind: u8) -> Self {
        Self(((protocol as u16) << 8) | (kind as u16))
    }

    /// Extract the protocol identifier.
    #[inline]
    pub const fn protocol(&self) -> u8 {
        (self.0 >> 8) as u8
    }

    /// Extract the kind identifier within the protocol namespace.
    #[inline]
    pub const fn kind(&self) -> u8 {
        self.0 as u8
    }

    /// Get the raw 16-bit key value.
    #[inline]
    pub const fn raw(&self) -> u16 {
        self.0
    }
}

/// Fixed-size context value (64-bit) for transport state.
///
/// Provides constructors and accessors for common types while maintaining
/// a compact representation suitable for `no_alloc` environments.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ContextValue(u64);

impl ContextValue {
    /// Sentinel value representing no data.
    pub const NONE: Self = Self(0);

    /// Create from a boolean value.
    #[inline]
    pub const fn from_bool(v: bool) -> Self {
        Self(v as u64)
    }

    /// Create from a u8 value.
    #[inline]
    pub const fn from_u8(v: u8) -> Self {
        Self(v as u64)
    }

    /// Create from a u16 value.
    #[inline]
    pub const fn from_u16(v: u16) -> Self {
        Self(v as u64)
    }

    /// Create from a u32 value.
    #[inline]
    pub const fn from_u32(v: u32) -> Self {
        Self(v as u64)
    }

    /// Create from a u64 value.
    #[inline]
    pub const fn from_u64(v: u64) -> Self {
        Self(v)
    }

    /// Create from a pair of u32 values (high, low).
    #[inline]
    pub const fn from_pair(hi: u32, lo: u32) -> Self {
        Self(((hi as u64) << 32) | (lo as u64))
    }

    /// Interpret as a boolean (non-zero = true).
    #[inline]
    pub const fn as_bool(&self) -> bool {
        self.0 != 0
    }

    /// Interpret as u8 (truncated).
    #[inline]
    pub const fn as_u8(&self) -> u8 {
        self.0 as u8
    }

    /// Interpret as u16 (truncated).
    #[inline]
    pub const fn as_u16(&self) -> u16 {
        self.0 as u16
    }

    /// Interpret as u32 (truncated).
    #[inline]
    pub const fn as_u32(&self) -> u32 {
        self.0 as u32
    }

    /// Interpret as u64.
    #[inline]
    pub const fn as_u64(&self) -> u64 {
        self.0
    }

    /// Interpret as a pair of u32 values (high, low).
    #[inline]
    pub const fn as_pair(&self) -> (u32, u32) {
        ((self.0 >> 32) as u32, self.0 as u32)
    }

    /// Check if the value is non-zero.
    #[inline]
    pub const fn is_some(&self) -> bool {
        self.0 != 0
    }

    /// Get the raw u64 value.
    #[inline]
    pub const fn raw(&self) -> u64 {
        self.0
    }
}

/// Protocol-agnostic transport context provider.
///
/// Transport implementations (QUIC, Raft, etc.) implement this trait to provide
/// O(1) access to protocol-specific state without global registries.
///
/// # Example
///
/// ```ignore
/// use hibana::transport::context::{TransportContextProvider, ContextKey, ContextValue, protocol};
///
/// // QUIC implementation defines its context keys
/// const STREAM_LOOP_KEY: ContextKey = ContextKey::new(protocol::QUIC, 0x20);
///
/// struct QuicContext { /* ... */ }
///
/// impl TransportContextProvider for QuicContext {
///     fn query(&self, key: ContextKey) -> Option<ContextValue> {
///         if key == STREAM_LOOP_KEY {
///             Some(ContextValue::from_bool(/* stream loop should continue */))
///         } else {
///             None
///         }
///     }
/// }
/// ```
pub trait TransportContextProvider {
    /// Query a context value by key.
    ///
    /// Returns `None` if the key is not supported or the value is unavailable.
    fn query(&self, key: ContextKey) -> Option<ContextValue>;

    /// Return the list of keys this provider supports.
    ///
    /// Used by `ContextSnapshot::from_provider()` to know which keys to query.
    /// Default returns an empty slice for backwards compatibility.
    fn supported_keys(&self) -> &[ContextKey] {
        &[]
    }
}

/// No-op context provider for situations where transport context is not needed.
///
/// All queries return `None`. This is useful as a default or placeholder.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoContext;

impl TransportContextProvider for NoContext {
    #[inline]
    fn query(&self, _key: ContextKey) -> Option<ContextValue> {
        None
    }
}

/// Fixed-size snapshot of transport context for resolver functions.
///
/// This struct captures a snapshot of transport state that can be passed
/// by value to resolver functions without requiring lifetimes. It holds
/// up to 16 key-value pairs and provides O(1) lookup by protocol/kind.
///
/// # Design Rationale
///
/// `ResolverContext` must be `Copy` to work with `fn` pointer types in
/// `DynamicResolverFn`. Instead of adding lifetime parameters (which would
/// require complex HRTB), we snapshot the relevant context values into this
/// fixed-size struct before calling the resolver.
///
/// Capacity of 16 supports QUIC's 14 context keys with room for future expansion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContextSnapshot {
    entries: [ContextEntry; CONTEXT_SNAPSHOT_CAPACITY],
    protocol_index: [u8; 256],
    kind_index: [[u8; 256]; CONTEXT_PROTOCOL_SLOTS],
    protocol_count: u8,
    len: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ContextEntry {
    key: ContextKey,
    value: ContextValue,
}

impl ContextEntry {
    const fn empty() -> Self {
        Self {
            key: ContextKey::new(protocol::RESERVED, 0),
            value: ContextValue::NONE,
        }
    }
}

impl Default for ContextSnapshot {
    fn default() -> Self {
        Self::new()
    }
}

impl ContextSnapshot {
    /// Maximum number of context entries.
    pub const CAPACITY: usize = CONTEXT_SNAPSHOT_CAPACITY;

    /// Create an empty context snapshot.
    #[inline]
    pub const fn new() -> Self {
        Self {
            entries: [ContextEntry::empty(); CONTEXT_SNAPSHOT_CAPACITY],
            protocol_index: [CONTEXT_INDEX_NONE; 256],
            kind_index: [[CONTEXT_INDEX_NONE; 256]; CONTEXT_PROTOCOL_SLOTS],
            protocol_count: 0,
            len: 0,
        }
    }

    /// Insert a key-value pair. Returns false if capacity is exhausted.
    #[inline]
    pub fn insert(&mut self, key: ContextKey, value: ContextValue) -> bool {
        if self.len as usize >= Self::CAPACITY {
            return false;
        }
        let protocol = key.protocol() as usize;
        let kind = key.kind() as usize;
        let slot = match self.protocol_index[protocol] {
            CONTEXT_INDEX_NONE => {
                let next = self.protocol_count as usize;
                if next >= CONTEXT_PROTOCOL_SLOTS {
                    return false;
                }
                self.protocol_index[protocol] = next as u8;
                self.protocol_count += 1;
                next
            }
            idx => idx as usize,
        };

        let existing = self.kind_index[slot][kind];
        if existing != CONTEXT_INDEX_NONE {
            self.entries[existing as usize].value = value;
            return true;
        }

        let entry_idx = self.len as usize;
        self.entries[entry_idx] = ContextEntry { key, value };
        self.kind_index[slot][kind] = entry_idx as u8;
        self.len += 1;
        true
    }

    /// Query a value by key.
    #[inline]
    pub fn query(&self, key: ContextKey) -> Option<ContextValue> {
        let protocol = key.protocol() as usize;
        let kind = key.kind() as usize;
        let slot = self.protocol_index[protocol];
        if slot == CONTEXT_INDEX_NONE {
            return None;
        }
        let entry_idx = self.kind_index[slot as usize][kind];
        if entry_idx == CONTEXT_INDEX_NONE {
            return None;
        }
        let entry = self.entries[entry_idx as usize];
        debug_assert_eq!(entry.key.raw(), key.raw());
        Some(entry.value)
    }

    /// Check if empty.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Get the number of entries.
    #[inline]
    pub const fn len(&self) -> usize {
        self.len as usize
    }

    /// Create a snapshot from a TransportContextProvider by querying specific keys.
    pub fn from_provider<P: TransportContextProvider + ?Sized>(
        provider: &P,
        keys: &[ContextKey],
    ) -> Self {
        let mut snapshot = Self::new();
        for &key in keys {
            if let Some(value) = provider.query(key) {
                if !snapshot.insert(key, value) {
                    break; // capacity exhausted
                }
            }
        }
        snapshot
    }
}

impl TransportContextProvider for ContextSnapshot {
    #[inline]
    fn query(&self, key: ContextKey) -> Option<ContextValue> {
        ContextSnapshot::query(self, key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_key_encoding() {
        let key = ContextKey::new(protocol::QUIC, 0x20);
        assert_eq!(key.protocol(), protocol::QUIC);
        assert_eq!(key.kind(), 0x20);
        assert_eq!(key.raw(), 0x0120);
    }

    #[test]
    fn context_value_conversions() {
        assert!(ContextValue::from_bool(true).as_bool());
        assert!(!ContextValue::from_bool(false).as_bool());

        assert_eq!(ContextValue::from_u8(42).as_u8(), 42);
        assert_eq!(ContextValue::from_u16(1234).as_u16(), 1234);
        assert_eq!(ContextValue::from_u32(0x12345678).as_u32(), 0x12345678);
        assert_eq!(
            ContextValue::from_u64(0x123456789ABCDEF0).as_u64(),
            0x123456789ABCDEF0
        );

        let pair = ContextValue::from_pair(0xAABBCCDD, 0x11223344);
        assert_eq!(pair.as_pair(), (0xAABBCCDD, 0x11223344));
    }

    #[test]
    fn no_context_returns_none() {
        let ctx = NoContext;
        let key = ContextKey::new(protocol::QUIC, 0x01);
        assert!(ctx.query(key).is_none());
    }

    #[test]
    fn context_snapshot_insert_query() {
        let mut snapshot = ContextSnapshot::new();
        assert!(snapshot.is_empty());

        let key1 = ContextKey::new(protocol::QUIC, 0x20);
        let key2 = ContextKey::new(protocol::QUIC, 0x21);
        let key3 = ContextKey::new(protocol::QUIC, 0x30);

        assert!(snapshot.insert(key1, ContextValue::from_bool(true)));
        assert!(snapshot.insert(key2, ContextValue::from_u8(42)));

        assert_eq!(snapshot.len(), 2);
        assert!(!snapshot.is_empty());

        assert!(snapshot.query(key1).unwrap().as_bool());
        assert_eq!(snapshot.query(key2).unwrap().as_u8(), 42);
        assert!(snapshot.query(key3).is_none());
    }

    #[test]
    fn context_snapshot_capacity() {
        let mut snapshot = ContextSnapshot::new();

        for i in 0..16u8 {
            let key = ContextKey::new(protocol::QUIC, i);
            assert!(snapshot.insert(key, ContextValue::from_u8(i)));
        }

        // 17th insert should fail
        let key17 = ContextKey::new(protocol::QUIC, 0xFF);
        assert!(!snapshot.insert(key17, ContextValue::from_u8(99)));

        // All 16 entries should be queryable
        for i in 0..16u8 {
            let key = ContextKey::new(protocol::QUIC, i);
            assert_eq!(snapshot.query(key).unwrap().as_u8(), i);
        }
    }

    #[test]
    fn context_snapshot_from_provider() {
        struct TestProvider;
        impl TransportContextProvider for TestProvider {
            fn query(&self, key: ContextKey) -> Option<ContextValue> {
                if key.kind() == 0x20 {
                    Some(ContextValue::from_bool(true))
                } else if key.kind() == 0x21 {
                    Some(ContextValue::from_u32(12345))
                } else {
                    None
                }
            }
        }

        let provider = TestProvider;
        let keys = [
            ContextKey::new(protocol::QUIC, 0x20),
            ContextKey::new(protocol::QUIC, 0x21),
            ContextKey::new(protocol::QUIC, 0x99), // not in provider
        ];

        let snapshot = ContextSnapshot::from_provider(&provider, &keys);
        assert_eq!(snapshot.len(), 2);
        assert!(snapshot.query(keys[0]).unwrap().as_bool());
        assert_eq!(snapshot.query(keys[1]).unwrap().as_u32(), 12345);
        assert!(snapshot.query(keys[2]).is_none());
    }

    #[test]
    fn context_snapshot_is_copy() {
        fn assert_copy<T: Copy>() {}
        assert_copy::<ContextSnapshot>();
    }
}
