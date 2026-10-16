pub(crate) mod meta {
    /// Number of event ordinals representable without using the reserved
    /// `u16::MAX` descriptor sentinel.
    pub(crate) const COMPACT_EVENT_IDENTITY_CAPACITY: usize = u16::MAX as usize;
}

/// Dense event position inside a projected local descriptor image.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct EffIndex(u16);

impl EffIndex {
    pub(crate) const ZERO: Self = Self(0);

    #[inline(always)]
    pub(crate) const fn from_dense_ordinal(idx: usize) -> Self {
        if idx >= meta::COMPACT_EVENT_IDENTITY_CAPACITY {
            crate::invariant();
        }
        Self(idx as u16)
    }

    #[inline(always)]
    pub(crate) const fn dense_ordinal(self) -> usize {
        self.0 as usize
    }
}

impl core::fmt::Display for EffIndex {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EventOrigin {
    User = 0,
    Session = 1,
}

impl EventOrigin {
    #[inline(always)]
    pub(crate) const fn decode_packed_bits(bits: u8) -> Option<Self> {
        match bits {
            0 => Some(Self::User),
            1 => Some(Self::Session),
            _ => None,
        }
    }

    #[inline(always)]
    pub(crate) const fn packed_bits(self) -> u8 {
        match self {
            Self::User => 0,
            Self::Session => 1,
        }
    }

    #[inline(always)]
    pub(crate) const fn is_session(self) -> bool {
        matches!(self, Self::Session)
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffAtom {
    pub(crate) from: u8,
    pub(crate) to: u8,
    pub(crate) label: u8,
    pub(crate) payload: EventPayload,
    pub(crate) origin: EventOrigin,
    /// Type-level lane for parallel composition; lane 0 is the primary lane.
    pub(crate) lane: u8,
}

/// The source payload kind survives lowering. An endpoint is never a wire
/// schema, including when a codec uses the same numeric value as its role.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EventPayload {
    Wire(u32),
    Endpoint(u8),
}

impl EventPayload {
    pub(crate) const OWNED_FLAG: u8 = 1 << 1;

    pub(crate) const fn encoded_value(self) -> u32 {
        match self {
            Self::Wire(schema) => schema,
            Self::Endpoint(role) => role as u32,
        }
    }

    pub(crate) const fn kind_bits(self) -> u8 {
        match self {
            Self::Wire(_) => 0,
            Self::Endpoint(_) => Self::OWNED_FLAG,
        }
    }

    pub(crate) const fn decode(value: u32, flags: u8, max_role: u8) -> Option<Self> {
        match flags & Self::OWNED_FLAG {
            0 => Some(Self::Wire(value)),
            _ if value <= max_role as u32 => Some(Self::Endpoint(value as u8)),
            _ => None,
        }
    }

    pub(crate) const fn selector_bits(self) -> u64 {
        match self {
            Self::Wire(schema) => schema as u64,
            Self::Endpoint(role) => (1u64 << 32) | role as u64,
        }
    }

    pub(crate) const fn wire_schema(self) -> u32 {
        match self {
            Self::Wire(schema) => schema,
            Self::Endpoint(_) => panic!("owned endpoint is not a wire payload"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::EffIndex;

    #[test]
    fn eff_index_from_dense_ordinal_is_dense() {
        let idx = EffIndex::from_dense_ordinal(42);

        assert_eq!(idx.dense_ordinal(), 42);
    }

    #[test]
    fn eff_index_display_is_dense() {
        let index = EffIndex::from_dense_ordinal(42);
        assert_eq!(std::format!("{index}"), "42");
    }

    #[test]
    #[should_panic]
    fn eff_index_rejects_reserved_descriptor_sentinel() {
        let _ = EffIndex::from_dense_ordinal(super::meta::COMPACT_EVENT_IDENTITY_CAPACITY);
    }
}
