#[cfg(all(test, hibana_repo_tests))]
use super::MAX_STATES;
use super::{StateIndex, checked_state_index};

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct FrontierVisitSet {
    slots: *mut StateIndex,
    capacity: u16,
    len: u16,
}

impl FrontierVisitSet {
    pub(crate) const EMPTY: Self = Self {
        slots: core::ptr::null_mut(),
        capacity: 0,
        len: 0,
    };

    #[inline]
    pub(crate) unsafe fn from_parts(slots: *mut StateIndex, capacity: usize) -> Self {
        if capacity > u16::MAX as usize || (capacity != 0 && slots.is_null()) {
            crate::invariant();
        }
        let mut idx = 0usize;
        while idx < capacity {
            /* SAFETY: `idx < capacity` bounds the resident visited-entry
            buffer. All cells are reset before `len` exposes the initialized
            prefix. */
            unsafe {
                slots.add(idx).write(StateIndex::ABSENT);
            }
            idx += 1;
        }
        Self {
            slots,
            capacity: capacity as u16,
            len: 0,
        }
    }

    #[inline]
    pub(crate) fn contains(&self, entry_idx: usize) -> bool {
        let Some(entry) = checked_state_index(entry_idx) else {
            crate::invariant();
        };
        let mut idx = 0usize;
        while idx < self.len as usize {
            if
            /* SAFETY: `idx < len` bounds the initialized prefix of the
            visited-entry buffer; this shared read copies one state identity. */
            unsafe { *self.slots.add(idx) } == entry {
                return true;
            }
            idx += 1;
        }
        false
    }

    #[inline]
    pub(crate) fn record(&mut self, entry_idx: usize) {
        if self.contains(entry_idx) {
            return;
        }
        if self.len >= self.capacity {
            crate::invariant();
        }
        let entry = crate::invariant_some(checked_state_index(entry_idx));
        /* SAFETY: `len < capacity` bounds the next visited-entry slot; the
        initialized prefix grows only after this write. */
        unsafe {
            self.slots.add(self.len as usize).write(entry);
        }
        self.len += 1;
    }

    #[inline]
    pub(crate) fn take(&mut self) -> Self {
        core::mem::replace(self, Self::EMPTY)
    }

    #[cfg(any(kani, all(test, hibana_repo_tests)))]
    #[inline]
    pub(crate) const fn len(&self) -> usize {
        self.len as usize
    }
}

#[cfg(all(test, hibana_repo_tests))]
mod tests {
    use super::*;

    #[test]
    #[should_panic]
    fn visit_set_fails_closed_instead_of_truncating() {
        let mut storage = [StateIndex::ABSENT; 1];
        /* SAFETY: `storage` is one initialized, live, exclusively borrowed
        visit cell whose exact length is passed to the view. */
        let mut visited =
            unsafe { FrontierVisitSet::from_parts(storage.as_mut_ptr(), storage.len()) };
        visited.record(1);
        visited.record(2);
    }

    #[test]
    fn visit_set_holds_the_current_entry_and_the_full_active_frontier() {
        const ACTIVE_LANE_COUNT: usize = u8::MAX as usize + 1;
        let mut storage = [StateIndex::ABSENT; ACTIVE_LANE_COUNT + 1];
        /* SAFETY: `storage` is initialized, live, and exclusively borrowed for
        the complete visit-set use. Its extra cell is the current cursor entry. */
        let mut visited =
            unsafe { FrontierVisitSet::from_parts(storage.as_mut_ptr(), storage.len()) };
        let current = MAX_STATES - 1;
        visited.record(current);
        let mut entry = 0usize;
        while entry < ACTIVE_LANE_COUNT {
            visited.record(entry);
            entry += 1;
        }

        assert_eq!(visited.len(), ACTIVE_LANE_COUNT + 1);
        assert!(visited.contains(current));
        assert!(visited.contains(ACTIVE_LANE_COUNT - 1));
    }

    #[test]
    fn absent_state_identity_is_not_admissible() {
        assert!(checked_state_index(MAX_STATES - 1).is_some());
        assert!(checked_state_index(MAX_STATES).is_none());
        assert!(checked_state_index(usize::MAX).is_none());
    }
}

#[cfg(kani)]
mod kani;
