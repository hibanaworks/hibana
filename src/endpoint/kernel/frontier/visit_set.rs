use crate::global::role_program::frontier_visit_byte_count;

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct FrontierVisitSet {
    position_bits: *mut u8,
    position_count: usize,
}

impl FrontierVisitSet {
    pub(crate) const EMPTY: Self = Self {
        position_bits: core::ptr::null_mut(),
        position_count: 0,
    };

    #[inline]
    pub(crate) unsafe fn from_parts(position_bits: *mut u8, position_count: usize) -> Self {
        if position_count > u16::MAX as usize + 1 {
            crate::invariant();
        }
        let byte_count = frontier_visit_byte_count(position_count);
        if byte_count != 0 && position_bits.is_null() {
            crate::invariant();
        }
        let mut byte_idx = 0usize;
        while byte_idx < byte_count {
            /* SAFETY: the endpoint arena owner supplies this exclusively
            borrowed resident bitmap; `byte_idx < byte_count` bounds each write,
            and every bit is cleared before the set can be observed. */
            unsafe {
                position_bits.add(byte_idx).write(0);
            }
            byte_idx += 1;
        }
        let position_bits = if byte_count == 0 {
            core::ptr::null_mut()
        } else {
            position_bits
        };
        Self {
            position_bits,
            position_count,
        }
    }

    #[inline]
    pub(crate) fn contains(&self, position_idx: usize) -> bool {
        if self.position_bits.is_null() {
            if self.position_count != 0 {
                crate::invariant();
            }
            return false;
        }
        if position_idx >= self.position_count {
            crate::invariant();
        }
        let byte_idx = position_idx / u8::BITS as usize;
        let mask = 1u8 << (position_idx % u8::BITS as usize);
        /* SAFETY: the endpoint arena owner keeps the initialized resident
        bitmap live; `position_idx < position_count` bounds `byte_idx`, and
        this shared read only copies one byte. */
        unsafe { *self.position_bits.add(byte_idx) & mask != 0 }
    }

    #[inline]
    pub(crate) fn record(&mut self, position_idx: usize) {
        if self.contains(position_idx) {
            return;
        }
        if self.position_bits.is_null() {
            crate::invariant();
        }
        let byte_idx = position_idx / u8::BITS as usize;
        let mask = 1u8 << (position_idx % u8::BITS as usize);
        /* SAFETY: `contains` established `position_idx < position_count`, and
        this mutable visit-set borrow exclusively owns the initialized resident
        bitmap byte bounded by `byte_idx`. */
        unsafe {
            *self.position_bits.add(byte_idx) |= mask;
        }
    }

    #[inline]
    pub(crate) fn take(&mut self) -> Self {
        core::mem::replace(self, Self::EMPTY)
    }

    #[cfg(any(kani, all(test, hibana_repo_tests)))]
    #[inline]
    pub(crate) fn len(&self) -> usize {
        let byte_count = frontier_visit_byte_count(self.position_count);
        let mut visited = 0usize;
        let mut byte_idx = 0usize;
        while byte_idx < byte_count {
            /* SAFETY: `byte_idx < byte_count` bounds the initialized bitmap
            byte, and this test/proof observer only reads it. */
            visited += unsafe { *self.position_bits.add(byte_idx) }.count_ones() as usize;
            byte_idx += 1;
        }
        visited
    }
}

#[cfg(all(test, hibana_repo_tests))]
mod tests {
    use super::*;

    fn visit_set<const N: usize>(storage: &mut [u8; N], position_count: usize) -> FrontierVisitSet {
        assert_eq!(frontier_visit_byte_count(position_count), N);
        /* SAFETY: the exact byte count for `position_count` equals the live,
        initialized, exclusively borrowed storage extent. */
        unsafe { FrontierVisitSet::from_parts(storage.as_mut_ptr(), position_count) }
    }

    #[test]
    #[should_panic]
    fn visit_set_fails_closed_instead_of_truncating() {
        let mut storage = [0u8; 1];
        let mut visited = visit_set(&mut storage, 1);
        visited.record(0);
        visited.record(1);
    }

    #[test]
    fn rolled_reentry_retains_every_distinct_cursor_position() {
        let mut storage = [0u8; 6];
        let mut visited = visit_set(&mut storage, 47);
        visited.record(46);
        visited.record(20);
        visited.record(2);

        assert_eq!(visited.len(), 3);
        assert!(visited.contains(46));
        assert!(visited.contains(20));
        assert!(visited.contains(2));
    }

    #[test]
    fn terminal_cursor_position_is_distinct_from_absent_event_identity() {
        let mut storage = [0u8; 8192];
        let mut visited = visit_set(&mut storage, u16::MAX as usize + 1);
        visited.record(u16::MAX as usize);

        assert!(visited.contains(u16::MAX as usize));
        assert_eq!(visited.len(), 1);
    }

    #[test]
    #[should_panic]
    fn visit_set_rejects_a_domain_beyond_compact_cursor_positions() {
        let mut storage = [0u8; 1];
        /* SAFETY: the constructor rejects the oversized domain before it can
        access the intentionally minimal storage. */
        let _ =
            unsafe { FrontierVisitSet::from_parts(storage.as_mut_ptr(), u16::MAX as usize + 2) };
    }
}

#[cfg(kani)]
mod kani;
