//! Route-state owner for endpoint kernel runtime bookkeeping.

use super::evidence::RouteArmState;
use super::frontier::{LaneOfferState, MAX_ROUTE_ARM_STACK};
use crate::global::const_dsl::ScopeId;
use crate::global::role_program::MAX_LANES;

#[cfg(feature = "std")]
fn boxed_repeat_array<T: Clone, const N: usize>(value: T) -> std::boxed::Box<[T; N]> {
    let values: std::boxed::Box<[T]> = std::vec![value; N].into_boxed_slice();
    match values.try_into() {
        Ok(fixed) => fixed,
        Err(_) => panic!("fixed array length"),
    }
}

pub(super) struct RouteState {
    #[cfg(feature = "std")]
    pub(super) lane_route_arms: std::boxed::Box<[[RouteArmState; MAX_ROUTE_ARM_STACK]; MAX_LANES]>,
    #[cfg(not(feature = "std"))]
    pub(super) lane_route_arms: [[RouteArmState; MAX_ROUTE_ARM_STACK]; MAX_LANES],
    #[cfg(feature = "std")]
    pub(super) lane_route_arm_lens: std::boxed::Box<[u8; MAX_LANES]>,
    #[cfg(not(feature = "std"))]
    pub(super) lane_route_arm_lens: [u8; MAX_LANES],
    #[cfg(feature = "std")]
    pub(super) lane_linger_counts: std::boxed::Box<[u8; MAX_LANES]>,
    #[cfg(not(feature = "std"))]
    pub(super) lane_linger_counts: [u8; MAX_LANES],
    pub(super) lane_linger_mask: u8,
    pub(super) lane_offer_linger_mask: u8,
    pub(super) active_offer_mask: u8,
    #[cfg(feature = "std")]
    pub(super) lane_offer_state: std::boxed::Box<[LaneOfferState; MAX_LANES]>,
    #[cfg(not(feature = "std"))]
    pub(super) lane_offer_state: [LaneOfferState; MAX_LANES],
}

impl RouteState {
    #[cfg(feature = "std")]
    pub(super) fn new() -> Self {
        Self {
            lane_route_arms: boxed_repeat_array([RouteArmState::EMPTY; MAX_ROUTE_ARM_STACK]),
            lane_route_arm_lens: boxed_repeat_array(0u8),
            lane_linger_counts: boxed_repeat_array(0u8),
            lane_linger_mask: 0,
            lane_offer_linger_mask: 0,
            active_offer_mask: 0,
            lane_offer_state: boxed_repeat_array(LaneOfferState::EMPTY),
        }
    }

    #[cfg(not(feature = "std"))]
    pub(super) fn new() -> Self {
        Self {
            lane_route_arms: [[RouteArmState::EMPTY; MAX_ROUTE_ARM_STACK]; MAX_LANES],
            lane_route_arm_lens: [0; MAX_LANES],
            lane_linger_counts: [0; MAX_LANES],
            lane_linger_mask: 0,
            lane_offer_linger_mask: 0,
            active_offer_mask: 0,
            lane_offer_state: [LaneOfferState::EMPTY; MAX_LANES],
        }
    }

    #[inline]
    pub(super) fn lane_route_arm_len(&self, lane_idx: usize) -> usize {
        self.lane_route_arm_lens.get(lane_idx).copied().unwrap_or(0) as usize
    }

    pub(super) fn set_route_arm(
        &mut self,
        lane_idx: usize,
        scope: ScopeId,
        arm: u8,
        is_linger: bool,
    ) -> Result<(), ()> {
        let len = self.lane_route_arm_len(lane_idx);
        for idx in 0..len {
            if self.lane_route_arms[lane_idx][idx].scope == scope {
                self.lane_route_arms[lane_idx][idx].arm = arm;
                return Ok(());
            }
        }

        if len >= MAX_ROUTE_ARM_STACK {
            return Err(());
        }
        self.lane_route_arms[lane_idx][len] = RouteArmState { scope, arm };
        self.lane_route_arm_lens[lane_idx] = self.lane_route_arm_lens[lane_idx].saturating_add(1);
        if is_linger {
            self.increment_linger_count(lane_idx);
        }
        Ok(())
    }

    pub(super) fn pop_route_arm(
        &mut self,
        lane_idx: usize,
        scope: ScopeId,
        is_linger: bool,
    ) -> bool {
        let len = self.lane_route_arm_len(lane_idx);
        if len == 0 {
            return false;
        }
        let Some(pos) = (0..len)
            .rev()
            .find(|&idx| self.lane_route_arms[lane_idx][idx].scope == scope)
        else {
            return false;
        };

        let last = len - 1;
        for idx in pos..last {
            self.lane_route_arms[lane_idx][idx] = self.lane_route_arms[lane_idx][idx + 1];
        }
        self.lane_route_arms[lane_idx][last] = RouteArmState::EMPTY;
        self.lane_route_arm_lens[lane_idx] = self.lane_route_arm_lens[lane_idx].saturating_sub(1);
        if is_linger {
            self.decrement_linger_count(lane_idx);
        }
        true
    }

    pub(super) fn collect_lane_scopes<F>(
        &self,
        lane_idx: usize,
        out: &mut [ScopeId; MAX_ROUTE_ARM_STACK],
        mut include: F,
    ) -> usize
    where
        F: FnMut(ScopeId) -> bool,
    {
        let len = self.lane_route_arm_len(lane_idx);
        let mut out_len = 0usize;
        let mut idx = 0usize;
        while idx < len {
            let scope = self.lane_route_arms[lane_idx][idx].scope;
            if include(scope) {
                out[out_len] = scope;
                out_len += 1;
            }
            idx += 1;
        }
        out_len
    }

    pub(super) fn route_arm_for(&self, lane_idx: usize, scope: ScopeId) -> Option<u8> {
        let len = self.lane_route_arm_len(lane_idx);
        (0..len).rev().find_map(|idx| {
            (self.lane_route_arms[lane_idx][idx].scope == scope)
                .then_some(self.lane_route_arms[lane_idx][idx].arm)
        })
    }

    pub(super) fn active_linger_scope_for_lane<F>(
        &self,
        lane_idx: usize,
        mut is_linger_route: F,
    ) -> Option<ScopeId>
    where
        F: FnMut(ScopeId) -> bool,
    {
        let len = self.lane_route_arm_len(lane_idx);
        let mut idx = len;
        while idx > 0 {
            idx -= 1;
            let slot = self.lane_route_arms[lane_idx][idx];
            if slot.scope.is_none() || slot.arm != 0 {
                continue;
            }
            if is_linger_route(slot.scope) {
                return Some(slot.scope);
            }
        }
        None
    }

    #[inline]
    pub(super) fn increment_linger_count(&mut self, lane_idx: usize) {
        let count = &mut self.lane_linger_counts[lane_idx];
        debug_assert!(*count < u8::MAX);
        *count = count.saturating_add(1);
        if *count == 1 {
            self.lane_linger_mask |= 1u8 << lane_idx;
        }
    }

    #[inline]
    pub(super) fn decrement_linger_count(&mut self, lane_idx: usize) {
        let count = &mut self.lane_linger_counts[lane_idx];
        debug_assert!(*count > 0);
        if *count == 0 {
            return;
        }
        *count = count.saturating_sub(1);
        if *count == 0 {
            self.lane_linger_mask &= !(1u8 << lane_idx);
        }
    }

    #[inline]
    pub(super) fn lane_offer_state(&self, lane_idx: usize) -> LaneOfferState {
        self.lane_offer_state
            .get(lane_idx)
            .copied()
            .unwrap_or(LaneOfferState::EMPTY)
    }

    #[inline]
    pub(super) fn clear_lane_offer_state(&mut self, lane_idx: usize) -> LaneOfferState {
        let bit = 1u8 << lane_idx;
        let old = self.lane_offer_state(lane_idx);
        if let Some(state) = self.lane_offer_state.get_mut(lane_idx) {
            *state = LaneOfferState::EMPTY;
        }
        self.active_offer_mask &= !bit;
        self.lane_offer_linger_mask &= !bit;
        old
    }

    #[inline]
    pub(super) fn set_lane_offer_state(
        &mut self,
        lane_idx: usize,
        info: LaneOfferState,
        is_linger: bool,
    ) {
        let bit = 1u8 << lane_idx;
        self.lane_offer_state[lane_idx] = info;
        self.active_offer_mask |= bit;
        if is_linger {
            self.lane_offer_linger_mask |= bit;
        } else {
            self.lane_offer_linger_mask &= !bit;
        }
    }
}
