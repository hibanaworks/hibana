//! First-recv dispatch mask for offer materialization.

use super::Arm;

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel) struct FirstRecvDispatchCache {
    arm_mask: u8,
}

impl FirstRecvDispatchCache {
    pub(in crate::endpoint::kernel) const EMPTY: Self = Self { arm_mask: 0 };

    #[inline]
    pub(in crate::endpoint::kernel) fn record(&mut self, arm_mask: u8) {
        if arm_mask & !0b11 != 0 {
            crate::invariant();
        }
        self.arm_mask = arm_mask;
    }

    #[inline]
    pub(in crate::endpoint::kernel) const fn arm_has_dispatch(&self, arm: Arm) -> bool {
        (self.arm_mask & (1u8 << arm.as_u8())) != 0
    }
}

#[cfg(test)]
mod tests {
    use super::{Arm, FirstRecvDispatchCache};

    #[test]
    fn first_recv_dispatch_preserves_exact_binary_mask() {
        for mask in 0..=0b11 {
            let mut cache = FirstRecvDispatchCache::EMPTY;
            cache.record(mask);
            assert_eq!(cache.arm_has_dispatch(Arm::LEFT), mask & 1 != 0);
            assert_eq!(cache.arm_has_dispatch(Arm::RIGHT), mask & 2 != 0);
        }
    }

    #[test]
    #[should_panic]
    fn first_recv_dispatch_rejects_out_of_domain_bits() {
        let mut cache = FirstRecvDispatchCache::EMPTY;
        cache.record(1 << 2);
    }
}
