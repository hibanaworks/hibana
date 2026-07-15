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
pub(crate) enum EffKind {
    Pure = 0,
    Atom = 1,
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
pub(crate) struct EffAtom {
    pub(crate) from: u8,
    pub(crate) to: u8,
    pub(crate) label: u8,
    pub(crate) payload_schema: u32,
    pub(crate) origin: EventOrigin,
    /// Type-level lane for parallel composition; lane 0 is the primary lane.
    pub(crate) lane: u8,
}

impl EffAtom {
    pub(crate) const ZERO: Self = Self {
        from: 0,
        to: 0,
        label: 0,
        payload_schema: 0,
        origin: EventOrigin::User,
        lane: 0,
    };
}

#[repr(transparent)]
#[derive(Clone, Copy)]
pub(crate) struct EffData {
    atom: EffAtom,
}

impl EffData {
    pub(crate) const fn empty() -> Self {
        Self {
            atom: EffAtom::ZERO,
        }
    }

    pub(crate) const fn from_atom(atom: EffAtom) -> Self {
        Self { atom }
    }

    #[inline(always)]
    pub(crate) const fn atom(&self) -> EffAtom {
        self.atom
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct EffStruct {
    pub(crate) kind: EffKind,
    pub(crate) data: EffData,
}

impl EffStruct {
    pub(crate) const fn pure() -> Self {
        Self {
            kind: EffKind::Pure,
            data: EffData::empty(),
        }
    }

    pub(crate) const fn atom(atom: EffAtom) -> Self {
        Self {
            kind: EffKind::Atom,
            data: EffData::from_atom(atom),
        }
    }

    #[inline(always)]
    pub(crate) const fn atom_data(&self) -> EffAtom {
        match self.kind {
            EffKind::Pure => crate::invariant(),
            EffKind::Atom => self.data.atom(),
        }
    }
}

impl core::fmt::Debug for EffStruct {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.kind {
            EffKind::Pure => f.debug_struct("EffStruct::Pure").finish(),
            EffKind::Atom => f
                .debug_struct("EffStruct::Atom")
                .field("atom", &self.atom_data())
                .finish(),
        }
    }
}

impl PartialEq for EffStruct {
    fn eq(&self, other: &Self) -> bool {
        if self.kind != other.kind {
            return false;
        }
        match self.kind {
            EffKind::Pure => true,
            EffKind::Atom => self.atom_data() == other.atom_data(),
        }
    }
}

impl Eq for EffStruct {}

#[cfg(test)]
mod tests {
    use super::{EffIndex, EffStruct};

    #[test]
    #[should_panic]
    fn pure_effect_atom_data_fails_fast() {
        let _ = EffStruct::pure().atom_data();
    }

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
