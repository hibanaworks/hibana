#[derive(Clone, Copy)]
pub(super) struct RouteEntry {
    pub(super) arm: u8,
    pub(super) observed_mask: u16,
}

impl RouteEntry {
    pub(super) const EMPTY: Self = Self {
        arm: 0,
        observed_mask: 0,
    };

    #[inline]
    pub(super) fn try_begin(self, observed_mask: u16, arm: u8) -> Option<Self> {
        if self.observed_mask != 0 || observed_mask == 0 || arm > 1 {
            None
        } else {
            Some(Self { arm, observed_mask })
        }
    }

    #[inline]
    pub(super) fn try_observe(self, observed_mask: u16, arm: u8) -> Option<Self> {
        if self.observed_mask == 0 || observed_mask == 0 || self.arm != arm {
            None
        } else {
            Some(Self {
                arm,
                observed_mask: self.observed_mask | observed_mask,
            })
        }
    }

    #[inline]
    pub(super) fn try_consume_role(self, role_bit: u16) -> Option<(Self, u8)> {
        if self.observed_mask == 0
            || role_bit == 0
            || (role_bit & (role_bit - 1)) != 0
            || (self.observed_mask & role_bit) != 0
        {
            None
        } else {
            Some((
                Self {
                    arm: self.arm,
                    observed_mask: self.observed_mask | role_bit,
                },
                self.arm,
            ))
        }
    }
}
