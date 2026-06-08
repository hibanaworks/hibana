//! First-recv dispatch mask for offer materialization.

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel) struct FirstRecvDispatchCache {
    arm_mask: u8,
}

impl FirstRecvDispatchCache {
    pub(in crate::endpoint::kernel) const EMPTY: Self = Self { arm_mask: 0 };

    #[inline]
    pub(in crate::endpoint::kernel) fn record(&mut self, arm_mask: u8) {
        self.arm_mask = arm_mask & 0b11;
    }

    #[inline]
    pub(in crate::endpoint::kernel) const fn arm_has_dispatch(&self, arm: u8) -> bool {
        arm < 2 && (self.arm_mask & (1u8 << arm)) != 0
    }
}
