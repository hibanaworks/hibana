//! Policy input and decision-observation attributes.
//!
//! Bindings project protocol-specific state into one `PolicyInput` value and
//! two optional core observations. Resolver authors read named accessors rather
//! than numeric slots or extension identifiers.

/// Decision-policy input word.
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

/// Policy signals provided by bindings.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PolicySignals {
    input: PolicyInput,
    attrs: PolicyAttrs,
}

impl PolicySignals {
    pub const ZERO: Self = Self::new(PolicyInput::ZERO, PolicyAttrs::EMPTY);

    #[inline]
    pub const fn new(input: PolicyInput, attrs: PolicyAttrs) -> Self {
        Self { input, attrs }
    }

    #[inline]
    pub const fn input(&self) -> PolicyInput {
        self.input
    }

    #[inline]
    pub const fn attrs(&self) -> &PolicyAttrs {
        &self.attrs
    }
}

/// Fixed-size resolver attribute value.
///
/// Only observations that are resolver-visible have storage here. Protocol-
/// specific values belong in [`PolicyInput`].
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PolicyAttrs {
    present: u8,
    queue_depth: u32,
    latency_us: u64,
}

impl Default for PolicyAttrs {
    fn default() -> Self {
        Self::new()
    }
}

impl PolicyAttrs {
    const LATENCY_PRESENT: u8 = 0b0000_0001;
    const QUEUE_DEPTH_PRESENT: u8 = 0b0000_0010;

    pub const EMPTY: Self = Self::new();

    /// Create an empty attribute value.
    #[inline]
    pub const fn new() -> Self {
        Self {
            present: 0,
            queue_depth: 0,
            latency_us: 0,
        }
    }

    #[inline]
    pub fn set_latency_us(&mut self, value: u64) {
        self.present |= Self::LATENCY_PRESENT;
        self.latency_us = value;
    }

    #[inline]
    pub fn set_queue_depth(&mut self, value: u32) {
        self.present |= Self::QUEUE_DEPTH_PRESENT;
        self.queue_depth = value;
    }

    #[inline]
    pub const fn latency_us(&self) -> Option<u64> {
        if (self.present & Self::LATENCY_PRESENT) != 0 {
            Some(self.latency_us)
        } else {
            None
        }
    }

    #[inline]
    pub const fn queue_depth(&self) -> Option<u32> {
        if (self.present & Self::QUEUE_DEPTH_PRESENT) != 0 {
            Some(self.queue_depth)
        } else {
            None
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
        fn mix_u32(mut hash: u32, value: u32) -> u32 {
            for byte in value.to_le_bytes() {
                hash = mix_u8(hash, byte);
            }
            hash
        }

        #[inline]
        fn mix_u64(mut hash: u32, value: u64) -> u32 {
            for byte in value.to_le_bytes() {
                hash = mix_u8(hash, byte);
            }
            hash
        }

        let mut hash = mix_u8(OFFSET, self.present);
        if let Some(value) = self.latency_us() {
            hash = mix_u8(hash, 0x10);
            hash = mix_u64(hash, value);
        }
        if let Some(value) = self.queue_depth() {
            hash = mix_u8(hash, 0x11);
            hash = mix_u32(hash, value);
        }
        hash
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_attrs_set_and_overwrite_values() {
        let mut attrs = PolicyAttrs::new();

        attrs.set_latency_us(1);
        attrs.set_queue_depth(2);
        attrs.set_latency_us(9);

        assert_eq!(attrs.len(), 2);
        assert_eq!(attrs.latency_us(), Some(9));
        assert_eq!(attrs.queue_depth(), Some(2));
    }

    #[test]
    fn policy_signals_zero_defaults() {
        let signals = PolicySignals::ZERO;
        assert_eq!(signals.input(), PolicyInput::ZERO);
        assert!(signals.attrs().is_empty());
    }

    #[test]
    fn policy_signals_carry_input_and_attrs() {
        fn attrs_with_queue_depth(value: u32) -> PolicyAttrs {
            let mut attrs = PolicyAttrs::EMPTY;
            attrs.set_queue_depth(value);
            attrs
        }

        let route = PolicySignals::new(PolicyInput::from_primary(1), attrs_with_queue_depth(1));
        let tx = PolicySignals::new(PolicyInput::from_primary(2), attrs_with_queue_depth(2));
        assert_eq!(route.input().primary(), 1);
        assert_eq!(tx.input().primary(), 2);
        assert_eq!(route.attrs().queue_depth(), Some(1));
        assert_eq!(tx.attrs().queue_depth(), Some(2));
    }

    #[test]
    fn policy_attrs_hash_changes_with_values() {
        let mut attrs_a = PolicyAttrs::EMPTY;
        attrs_a.set_queue_depth(1);
        let mut attrs_b = PolicyAttrs::EMPTY;
        attrs_b.set_queue_depth(2);
        assert_ne!(attrs_a.hash32(), attrs_b.hash32());
    }
}
