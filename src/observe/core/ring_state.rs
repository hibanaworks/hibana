use super::TAP_EVENTS;

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum HeadEra {
    Initial,
    Wrapped,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct RingState {
    write_index: u8,
    resident_len: u8,
    head_era: HeadEra,
}

impl RingState {
    pub(super) const EMPTY: Self = Self {
        write_index: 0,
        resident_len: 0,
        head_era: HeadEra::Initial,
    };

    #[inline(always)]
    pub(super) fn after_push(self, head: usize) -> Self {
        let write_index = if self.write_index as usize + 1 == TAP_EVENTS {
            0
        } else {
            self.write_index + 1
        };
        Self {
            write_index,
            resident_len: if self.resident_len as usize == TAP_EVENTS {
                self.resident_len
            } else {
                self.resident_len + 1
            },
            head_era: if head == usize::MAX {
                HeadEra::Wrapped
            } else {
                self.head_era
            },
        }
    }

    #[inline(always)]
    pub(super) fn oldest_index(self) -> u8 {
        ((self.write_index as usize + TAP_EVENTS - self.resident_len as usize) % TAP_EVENTS) as u8
    }

    #[inline(always)]
    pub(super) const fn write_index(self) -> u8 {
        self.write_index
    }

    #[inline(always)]
    pub(super) const fn resident_len(self) -> u8 {
        self.resident_len
    }

    #[inline(always)]
    pub(super) const fn head_era(self) -> HeadEra {
        self.head_era
    }
}

#[cfg(kani)]
mod kani {
    use super::{HeadEra, RingState, TAP_EVENTS};

    #[kani::proof]
    fn ring_state_step_preserves_the_exact_slot_domain() {
        let write_index: u8 = kani::any();
        let resident_len: u8 = kani::any();
        kani::assume((write_index as usize) < TAP_EVENTS);
        kani::assume(resident_len as usize <= TAP_EVENTS);
        let head_era = if kani::any() {
            HeadEra::Initial
        } else {
            HeadEra::Wrapped
        };
        let head: usize = kani::any();
        let state = RingState {
            write_index,
            resident_len,
            head_era,
        };

        let next = state.after_push(head);

        assert!((next.write_index as usize) < TAP_EVENTS);
        assert!(next.resident_len as usize <= TAP_EVENTS);
        assert!((next.oldest_index() as usize) < TAP_EVENTS);
        assert!(!matches!(head_era, HeadEra::Wrapped) || matches!(next.head_era, HeadEra::Wrapped));
        assert!(head != usize::MAX || matches!(next.head_era, HeadEra::Wrapped));
    }
}
