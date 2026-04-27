pub(crate) mod meta {
    /// Maximum number of effect rows stored in one lowering segment.
    pub(crate) const MAX_SEGMENT_EFFS: usize = 96;

    /// Maximum number of fixed lowering segments in one program image.
    pub(crate) const MAX_SEGMENTS: usize = 32;

    /// Total effect row capacity across all lowering segments.
    pub(crate) const MAX_EFF_NODES: usize = MAX_SEGMENTS * MAX_SEGMENT_EFFS;
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EffIndex(u32);

impl EffIndex {
    pub const ZERO: Self = Self(0);
    pub const MAX: Self = Self(u32::MAX);

    #[inline(always)]
    pub(crate) const fn from_segment_offset(segment: u16, offset: u16) -> Self {
        if segment as usize >= meta::MAX_SEGMENTS || offset as usize >= meta::MAX_SEGMENT_EFFS {
            panic!("segmented eff index out of bounds");
        }
        Self(((segment as u32) << 16) | offset as u32)
    }

    #[inline(always)]
    pub(crate) const fn from_dense_ordinal(idx: usize) -> Self {
        if idx >= meta::MAX_EFF_NODES {
            panic!("eff index exceeds segmented program capacity");
        }
        let segment = idx / meta::MAX_SEGMENT_EFFS;
        let offset = idx % meta::MAX_SEGMENT_EFFS;
        Self::from_segment_offset(segment as u16, offset as u16)
    }

    #[inline(always)]
    pub const fn raw(self) -> u32 {
        self.0
    }

    #[inline(always)]
    pub(crate) const fn segment(self) -> u16 {
        (self.0 >> 16) as u16
    }

    #[inline(always)]
    pub(crate) const fn offset(self) -> u16 {
        self.0 as u16
    }

    #[inline(always)]
    pub const fn as_usize(self) -> usize {
        if self.raw() == Self::MAX.raw() {
            panic!("sentinel eff index cannot be represented as a program offset");
        }
        let segment = self.segment() as usize;
        let offset = self.offset() as usize;
        if segment >= meta::MAX_SEGMENTS || offset >= meta::MAX_SEGMENT_EFFS {
            panic!("segmented eff index is outside the fixed program image");
        }
        segment * meta::MAX_SEGMENT_EFFS + offset
    }
}

impl core::fmt::Display for EffIndex {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if self.segment() == 0 {
            write!(f, "{}", self.offset())
        } else {
            write!(f, "{}:{}", self.segment(), self.offset())
        }
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffKind {
    Pure = 0,
    Atom = 1,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffAtom {
    pub from: u8,
    pub to: u8,
    pub label: u8,
    pub is_control: bool,
    pub resource: Option<u8>,
    /// Type-level lane for parallel composition (default 0).
    pub lane: u8,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub union EffData {
    pub atom: EffAtom,
    pub empty: (),
}

impl EffData {
    pub const fn empty() -> Self {
        Self { empty: () }
    }

    pub const fn from_atom(atom: EffAtom) -> Self {
        Self { atom }
    }

    #[inline(always)]
    pub const fn atom(&self) -> EffAtom {
        unsafe { self.atom }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct EffStruct {
    pub kind: EffKind,
    pub data: EffData,
}

impl EffStruct {
    pub const fn pure() -> Self {
        Self {
            kind: EffKind::Pure,
            data: EffData::empty(),
        }
    }

    pub const fn atom(atom: EffAtom) -> Self {
        Self {
            kind: EffKind::Atom,
            data: EffData::from_atom(atom),
        }
    }

    #[inline(always)]
    pub const fn atom_data(&self) -> EffAtom {
        self.data.atom()
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
    use super::EffIndex;

    #[test]
    fn eff_index_checked_constructor_packs_valid_segment_and_offset() {
        let idx = EffIndex::from_segment_offset(2, 42);

        assert_eq!(idx.segment(), 2);
        assert_eq!(idx.offset(), 42);
        assert_eq!(idx.raw(), (2u32 << 16) | 42);
    }

    #[test]
    #[should_panic(expected = "segmented eff index out of bounds")]
    fn invalid_eff_index_constructor_panics_at_construction() {
        let _ = EffIndex::from_segment_offset(super::meta::MAX_SEGMENTS as u16, 0);
    }

    #[test]
    fn eff_index_from_dense_ordinal_stays_segment_zero() {
        let idx = EffIndex::from_dense_ordinal(42);

        assert_eq!(idx.segment(), 0);
        assert_eq!(idx.offset(), 42);
        assert_eq!(idx.as_usize(), 42);
    }

    #[test]
    fn eff_index_from_dense_ordinal_crosses_segment_boundary() {
        let idx = EffIndex::from_dense_ordinal(super::meta::MAX_SEGMENT_EFFS + 7);

        assert_eq!(idx.segment(), 1);
        assert_eq!(idx.offset(), 7);
        assert_eq!(idx.as_usize(), super::meta::MAX_SEGMENT_EFFS + 7);
    }
}
