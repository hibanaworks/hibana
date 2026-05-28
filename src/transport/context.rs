//! Policy input and attribute contracts.
//!
//! This module exposes route-policy [`PolicySignals`] (`PolicyInput` + `attrs`).
//!
const CORE_ATTR_COUNT: usize = 6;

const CORE_ATTR_IDS: [ContextId; CORE_ATTR_COUNT] = [
    core::RV_ID,
    core::SESSION_ID,
    core::LANE,
    core::TAG,
    core::LATENCY_US,
    core::QUEUE_DEPTH,
];

/// Reserved core context identifiers surfaced through `ResolverContext::attr()`.
pub(crate) mod core {
    use super::ContextId;

    const CORE_CONTEXT_NAMESPACE: u16 = 0x0000;

    #[inline]
    const fn core_context_id(kind: u8) -> ContextId {
        ContextId::core(CORE_CONTEXT_NAMESPACE | kind as u16)
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
}

/// Internal identifier for runtime-owned policy context entries.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct ContextId(u16);

impl ContextId {
    #[inline]
    const fn core(raw: u16) -> Self {
        Self(raw)
    }

    #[inline]
    pub(crate) const fn raw(self) -> u16 {
        self.0
    }
}

/// Route-policy input word.
///
/// Resolver-facing policy input is intentionally a single named value. Richer
/// protocol state should be projected by the binding into this primary value
/// before resolver invocation instead of exposing numeric slots.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PolicyInput {
    primary: u32,
}

impl PolicyInput {
    pub const ZERO: Self = Self { primary: 0 };

    #[inline]
    pub const fn from_primary(primary: u32) -> Self {
        Self { primary }
    }

    #[inline]
    pub(crate) const fn replay_words(self) -> [u32; 4] {
        [self.primary, 0, 0, 0]
    }

    #[inline]
    pub const fn primary(self) -> u32 {
        self.primary
    }
}

/// Fixed-size value for runtime-owned context state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ContextValue(u64);

impl ContextValue {
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
    pub const fn as_u32(self) -> u32 {
        self.0 as u32
    }

    #[inline]
    pub const fn as_u64(self) -> u64 {
        self.0
    }

    #[inline]
    pub const fn raw(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug)]
enum PolicyAttrsBacking<'a> {
    Borrowed(&'a PolicyAttrs),
    Owned(PolicyAttrs),
}

impl<'a> PolicyAttrsBacking<'a> {
    #[inline]
    fn as_ref(&self) -> &PolicyAttrs {
        match self {
            Self::Borrowed(attrs) => attrs,
            Self::Owned(attrs) => attrs,
        }
    }
}

/// Policy signals provided by bindings.
#[derive(Clone, Copy, Debug)]
pub struct PolicySignals<'a> {
    input: PolicyInput,
    attrs: PolicyAttrsBacking<'a>,
}

impl<'a> PolicySignals<'a> {
    #[inline]
    pub const fn borrowed(input: PolicyInput, attrs: &'a PolicyAttrs) -> Self {
        Self {
            input,
            attrs: PolicyAttrsBacking::Borrowed(attrs),
        }
    }

    #[inline]
    pub const fn owned(input: PolicyInput, attrs: PolicyAttrs) -> Self {
        Self {
            input,
            attrs: PolicyAttrsBacking::Owned(attrs),
        }
    }

    #[inline]
    pub const fn input(&self) -> PolicyInput {
        self.input
    }

    #[inline]
    pub fn attrs(&self) -> &PolicyAttrs {
        self.attrs.as_ref()
    }

    #[inline]
    pub fn into_owned(self) -> PolicySignals<'static> {
        PolicySignals::owned(self.input, *self.attrs())
    }
}

impl PolicySignals<'static> {
    pub const ZERO: Self = Self::borrowed(PolicyInput::ZERO, &PolicyAttrs::EMPTY);
}

impl PartialEq for PolicySignals<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.input == other.input && self.attrs() == other.attrs()
    }
}

impl Eq for PolicySignals<'_> {}

impl Default for PolicySignals<'_> {
    fn default() -> Self {
        Self::borrowed(PolicyInput::ZERO, &PolicyAttrs::EMPTY)
    }
}

/// Fixed-size policy attribute map passed by value.
///
/// Runtime-owned attributes live in a packed bitset + value array for O(1)
/// lookup. Protocol-specific observations do not belong in this namespace;
/// integrations should project them into [`PolicyInput`] before resolver
/// invocation.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PolicyAttrs {
    present: u32,
    core_values: [u64; CORE_ATTR_COUNT],
}

impl Default for PolicyAttrs {
    fn default() -> Self {
        Self::new()
    }
}

impl PolicyAttrs {
    pub const EMPTY: Self = Self::new();

    /// Create an empty attribute map.
    #[inline]
    pub const fn new() -> Self {
        Self {
            present: 0,
            core_values: [0; CORE_ATTR_COUNT],
        }
    }

    #[inline]
    pub(crate) fn insert_core(&mut self, id: ContextId, value: ContextValue) {
        if let Some(idx) = core_attr_index(id) {
            self.present |= 1u32 << idx;
            self.core_values[idx] = value.raw();
        }
    }

    #[inline]
    pub fn set_latency_us(&mut self, value: u64) {
        self.insert_core(core::LATENCY_US, ContextValue::from_u64(value));
    }

    #[inline]
    pub fn set_queue_depth(&mut self, value: u32) {
        self.insert_core(core::QUEUE_DEPTH, ContextValue::from_u32(value));
    }

    #[inline]
    pub fn with_latency_us(mut self, value: u64) -> Self {
        self.set_latency_us(value);
        self
    }

    #[inline]
    pub fn with_queue_depth(mut self, value: u32) -> Self {
        self.set_queue_depth(value);
        self
    }

    #[inline]
    pub const fn latency_us(&self) -> Option<u64> {
        match self.get(core::LATENCY_US) {
            Some(value) => Some(value.as_u64()),
            None => None,
        }
    }

    #[inline]
    pub const fn queue_depth(&self) -> Option<u32> {
        match self.get(core::QUEUE_DEPTH) {
            Some(value) => Some(value.as_u32()),
            None => None,
        }
    }

    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.present == 0
    }

    #[inline]
    pub const fn len(&self) -> usize {
        self.present.count_ones() as usize
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

        let mut hash = mix_u8(OFFSET, self.present.count_ones() as u8);

        let mut idx = 0usize;
        while idx < CORE_ATTR_COUNT {
            if (self.present & (1u32 << idx)) != 0 {
                hash = mix_u16(hash, CORE_ATTR_IDS[idx].raw());
                hash = mix_u64(hash, self.core_values[idx]);
            }
            idx += 1;
        }

        hash
    }

    /// Copy attributes from another map.
    #[inline]
    pub fn copy_from(&mut self, src: &PolicyAttrs) {
        self.present |= src.present;
        let mut idx = 0usize;
        while idx < CORE_ATTR_COUNT {
            if (src.present & (1u32 << idx)) != 0 {
                self.core_values[idx] = src.core_values[idx];
            }
            idx += 1;
        }
    }

    #[inline]
    pub(crate) const fn get(&self, id: ContextId) -> Option<ContextValue> {
        if let Some(idx) = core_attr_index(id) {
            let bit = 1u32 << idx;
            if (self.present & bit) != 0 {
                return Some(ContextValue::from_u64(self.core_values[idx]));
            }
            return None;
        }

        None
    }
}

#[inline]
const fn core_attr_index(id: ContextId) -> Option<usize> {
    match id.raw() {
        raw if raw == core::RV_ID.raw() => Some(0),
        raw if raw == core::SESSION_ID.raw() => Some(1),
        raw if raw == core::LANE.raw() => Some(2),
        raw if raw == core::TAG.raw() => Some(3),
        raw if raw == core::LATENCY_US.raw() => Some(4),
        raw if raw == core::QUEUE_DEPTH.raw() => Some(5),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_context_id_roundtrip() {
        assert_eq!(core::TAG.raw(), 0x0004);
    }

    #[test]
    fn context_value_conversions() {
        assert_eq!(ContextValue::from_u32(0x1234_5678).as_u32(), 0x1234_5678);
        assert_eq!(
            ContextValue::from_u64(0x1234_5678_9ABC_DEF0).as_u64(),
            0x1234_5678_9ABC_DEF0
        );
    }

    #[test]
    fn policy_attrs_insert_get_and_overwrite_core_values() {
        let mut attrs = PolicyAttrs::new();

        attrs.set_latency_us(1);
        attrs.set_queue_depth(2);
        attrs.set_latency_us(9);

        assert_eq!(attrs.len(), 2);
        assert_eq!(attrs.latency_us(), Some(9));
        assert_eq!(attrs.queue_depth(), Some(2));
        assert!(attrs.get(ContextId::default()).is_none());
    }

    #[test]
    fn policy_signals_zero_defaults() {
        let signals = PolicySignals::ZERO;
        assert_eq!(signals.input(), PolicyInput::ZERO);
        assert!(signals.attrs().is_empty());
    }

    #[test]
    fn policy_signals_carry_input_and_attrs() {
        let mut route_attrs = PolicyAttrs::new();
        route_attrs.set_queue_depth(1);
        let mut tx_attrs = PolicyAttrs::new();
        tx_attrs.set_queue_depth(2);

        let route = PolicySignals::owned(PolicyInput::from_primary(1), route_attrs);
        let tx = PolicySignals::owned(PolicyInput::from_primary(2), tx_attrs);
        assert_eq!(route.input().primary(), 1);
        assert_eq!(tx.input().primary(), 2);
        assert_eq!(route.attrs().queue_depth(), Some(1));
        assert_eq!(tx.attrs().queue_depth(), Some(2));
    }

    #[test]
    fn policy_attrs_copy_from_merges_core_values() {
        let mut src = PolicyAttrs::new();
        src.set_latency_us(7);
        src.set_queue_depth(9);
        let mut dst = PolicyAttrs::new();
        dst.set_queue_depth(1);
        dst.copy_from(&src);
        assert_eq!(dst.latency_us(), Some(7));
        assert_eq!(dst.queue_depth(), Some(9));
    }

    #[test]
    fn policy_attrs_hash_changes_with_values() {
        let mut attrs_a = PolicyAttrs::new();
        let mut attrs_b = PolicyAttrs::new();
        attrs_a.set_queue_depth(1);
        attrs_b.set_queue_depth(2);
        assert_ne!(attrs_a.hash32(), attrs_b.hash32());
    }
}
