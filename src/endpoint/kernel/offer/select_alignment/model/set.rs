use super::super::super::ObservedEntrySet;

#[derive(Clone, Copy)]
struct OfferEntryMask {
    bits: u8,
}

impl OfferEntryMask {
    #[inline]
    const fn empty() -> Self {
        Self { bits: 0 }
    }

    #[inline]
    const fn from_bits(bits: u8) -> Self {
        Self { bits }
    }

    #[inline]
    const fn slot(slot_idx: usize) -> Self {
        Self {
            bits: 1u8 << slot_idx,
        }
    }

    #[inline]
    const fn is_empty(self) -> bool {
        self.bits == 0
    }

    #[inline]
    const fn has_one(self) -> bool {
        self.bits.count_ones() == 1
    }

    #[inline]
    const fn intersects(self, other: Self) -> bool {
        (self.bits & other.bits) != 0
    }

    #[inline]
    const fn union(self, other: Self) -> Self {
        Self {
            bits: self.bits | other.bits,
        }
    }

    #[inline]
    const fn intersect(self, other: Self) -> Self {
        Self {
            bits: self.bits & other.bits,
        }
    }

    #[inline]
    const fn without(self, other: Self) -> Self {
        Self {
            bits: self.bits & !other.bits,
        }
    }

    #[inline]
    const fn retain_singleton(self) -> Self {
        if self.has_one() { self } else { Self::empty() }
    }

    #[inline]
    fn first_entry_idx(self, observed_entries: ObservedEntrySet) -> Option<usize> {
        observed_entries.first_entry_idx(self.bits)
    }
}

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel::offer::select_alignment) struct OfferEntrySet {
    storage: OfferEntryMask,
}

impl OfferEntrySet {
    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn empty() -> Self {
        Self {
            storage: OfferEntryMask::empty(),
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn from_bits(bits: u8) -> Self {
        Self {
            storage: OfferEntryMask::from_bits(bits),
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn slot(
        slot_idx: usize,
    ) -> Self {
        Self {
            storage: OfferEntryMask::slot(slot_idx),
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn is_empty(self) -> bool {
        self.storage.is_empty()
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn has_one(self) -> bool {
        self.storage.has_one()
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn intersects(
        self,
        other: Self,
    ) -> bool {
        self.storage.intersects(other.storage)
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn union(
        self,
        other: Self,
    ) -> Self {
        Self {
            storage: self.storage.union(other.storage),
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn intersect(
        self,
        other: Self,
    ) -> Self {
        Self {
            storage: self.storage.intersect(other.storage),
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn without(
        self,
        other: Self,
    ) -> Self {
        Self {
            storage: self.storage.without(other.storage),
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn retain_singleton(
        self,
    ) -> Self {
        Self {
            storage: self.storage.retain_singleton(),
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) fn first_entry_idx(
        self,
        observed_entries: ObservedEntrySet,
    ) -> Option<usize> {
        self.storage.first_entry_idx(observed_entries)
    }
}
