/// Immutable source interval and resident I/O slot for a scoped borrow.
/// This carries no cursor, choice, holder, visit or completion state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct OperationRegion {
    start: u16,
    end: u16,
    lane: u16,
}

impl OperationRegion {
    pub(crate) const WHOLE: Self = Self {
        start: 0,
        end: u16::MAX,
        lane: 256,
    };

    pub(crate) fn parallel(start: usize, end: usize, lane: usize) -> Option<Self> {
        if start >= end || lane > u8::MAX as usize {
            return None;
        }
        Some(Self {
            start: u16::try_from(start).ok()?,
            end: u16::try_from(end).ok()?,
            lane: lane as u16,
        })
    }

    pub(crate) const fn is_whole(self) -> bool {
        self.lane == 256
    }

    pub(crate) const fn lane(self, primary: usize) -> usize {
        if self.is_whole() {
            primary
        } else {
            self.lane as usize
        }
    }

    pub(crate) const fn contains(self, source: usize) -> bool {
        self.start as usize <= source && source < self.end as usize
    }

    pub(crate) fn bounds(self) -> core::ops::Range<usize> {
        usize::from(self.start)..usize::from(self.end)
    }
}
