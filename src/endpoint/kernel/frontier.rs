//! Frontier-selection helpers for `offer()`.

use core::{convert::TryFrom, future::poll_fn, task::Poll};

use super::evidence::{ScopeLabelMeta, ScopeLoopMeta};
use super::offer::{CurrentScopeSelectionMeta, ScopeArmMaterializationMeta};
use crate::global::const_dsl::ScopeId;
use crate::global::role_program::MAX_LANES;
use crate::global::typestate::{MAX_STATES, StateIndex, state_index_to_usize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FrontierKind {
    Route,
    Loop,
    Parallel,
    PassiveObserver,
}

impl FrontierKind {
    #[inline]
    pub(super) const fn as_audit_tag(self) -> u8 {
        match self {
            Self::Route => 1,
            Self::Loop => 2,
            Self::Parallel => 3,
            Self::PassiveObserver => 4,
        }
    }

    #[inline]
    pub(super) const fn bit(self) -> u8 {
        match self {
            Self::Route => 1 << 0,
            Self::Loop => 1 << 1,
            Self::Parallel => 1 << 2,
            Self::PassiveObserver => 1 << 3,
        }
    }
}

#[inline]
pub(super) fn checked_state_index(idx: usize) -> Option<StateIndex> {
    u16::try_from(idx).ok().map(StateIndex::new)
}

#[derive(Clone, Copy)]
pub(super) struct LaneOfferState {
    pub(super) scope: ScopeId,
    pub(super) entry: StateIndex,
    pub(super) parallel_root: ScopeId,
    pub(super) frontier: FrontierKind,
    pub(super) loop_meta: ScopeLoopMeta,
    pub(super) label_meta: ScopeLabelMeta,
    pub(super) static_ready: bool,
    pub(super) flags: u8,
}

impl LaneOfferState {
    pub(super) const FLAG_CONTROLLER: u8 = 1;
    pub(super) const FLAG_DYNAMIC: u8 = 1 << 1;
    pub(super) const EMPTY: Self = Self {
        scope: ScopeId::none(),
        entry: StateIndex::MAX,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::Route,
        loop_meta: ScopeLoopMeta::EMPTY,
        label_meta: ScopeLabelMeta::EMPTY,
        static_ready: false,
        flags: 0,
    };

    #[inline]
    pub(super) fn is_controller(self) -> bool {
        (self.flags & Self::FLAG_CONTROLLER) != 0
    }

    #[inline]
    pub(super) fn is_dynamic(self) -> bool {
        (self.flags & Self::FLAG_DYNAMIC) != 0
    }

    #[inline]
    pub(super) fn static_ready(self) -> bool {
        self.static_ready
    }
}
#[derive(Clone, Copy)]
pub(super) struct ActiveEntrySet {
    pub(super) len: u8,
    pub(super) entries: [StateIndex; MAX_LANES],
    pub(super) lane_idx: [u8; MAX_LANES],
}

impl ActiveEntrySet {
    pub(super) const EMPTY: Self = Self {
        len: 0,
        entries: [StateIndex::MAX; MAX_LANES],
        lane_idx: [u8::MAX; MAX_LANES],
    };

    #[inline]
    pub(super) fn occupancy_mask(self) -> u8 {
        let len = self.len as usize;
        if len >= MAX_LANES {
            u8::MAX
        } else {
            (1u8 << len) - 1
        }
    }

    #[inline]
    pub(super) fn entry_at(self, slot_idx: usize) -> Option<usize> {
        if slot_idx >= self.len as usize {
            return None;
        }
        Some(state_index_to_usize(self.entries[slot_idx]))
    }

    #[inline]
    pub(super) fn contains_only(self, entry_idx: usize) -> bool {
        self.len == 1 && self.entry_at(0) == Some(entry_idx)
    }

    #[inline]
    pub(super) fn slot_for_entry(self, entry_idx: usize) -> Option<usize> {
        let entry = checked_state_index(entry_idx)?;
        let len = self.len as usize;
        let mut slot_idx = 0usize;
        while slot_idx < len {
            if self.entries[slot_idx] == entry {
                return Some(slot_idx);
            }
            slot_idx += 1;
        }
        None
    }

    pub(super) fn insert_entry(&mut self, entry_idx: usize, lane_idx: u8) -> bool {
        let Some(entry) = checked_state_index(entry_idx) else {
            return false;
        };
        let len = self.len as usize;
        let mut insert_idx = 0usize;
        while insert_idx < len {
            if self.entries[insert_idx] == entry {
                return false;
            }
            let existing_lane_idx = self.lane_idx[insert_idx];
            let existing_entry = self.entries[insert_idx];
            if existing_lane_idx > lane_idx
                || (existing_lane_idx == lane_idx && existing_entry.raw() > entry.raw())
            {
                break;
            }
            insert_idx += 1;
        }
        if len >= MAX_LANES {
            return false;
        }
        let mut shift_idx = len;
        while shift_idx > insert_idx {
            self.entries[shift_idx] = self.entries[shift_idx - 1];
            self.lane_idx[shift_idx] = self.lane_idx[shift_idx - 1];
            shift_idx -= 1;
        }
        self.entries[insert_idx] = entry;
        self.lane_idx[insert_idx] = lane_idx;
        self.len += 1;
        true
    }

    pub(super) fn remove_entry(&mut self, entry_idx: usize) -> bool {
        let Ok(entry) = u16::try_from(entry_idx) else {
            return false;
        };
        let len = self.len as usize;
        let mut idx = 0usize;
        while idx < len {
            if self.entries[idx] == entry {
                break;
            }
            idx += 1;
        }
        if idx >= len {
            return false;
        }
        while idx + 1 < len {
            self.entries[idx] = self.entries[idx + 1];
            self.lane_idx[idx] = self.lane_idx[idx + 1];
            idx += 1;
        }
        self.entries[len - 1] = StateIndex::MAX;
        self.lane_idx[len - 1] = u8::MAX;
        self.len = self.len.saturating_sub(1);
        true
    }
}

#[derive(Clone, Copy)]
pub(super) struct ObservedEntrySet {
    pub(super) len: u8,
    pub(super) entries: [StateIndex; MAX_LANES],
    pub(super) slot_by_entry: [u8; MAX_STATES],
    pub(super) controller_mask: u8,
    pub(super) dynamic_controller_mask: u8,
    pub(super) progress_mask: u8,
    pub(super) ready_arm_mask: u8,
    pub(super) ready_mask: u8,
    pub(super) route_mask: u8,
    pub(super) parallel_mask: u8,
    pub(super) loop_mask: u8,
    pub(super) passive_observer_mask: u8,
}

impl ObservedEntrySet {
    pub(super) const EMPTY: Self = Self {
        len: 0,
        entries: [StateIndex::MAX; MAX_LANES],
        slot_by_entry: [u8::MAX; MAX_STATES],
        controller_mask: 0,
        dynamic_controller_mask: 0,
        progress_mask: 0,
        ready_arm_mask: 0,
        ready_mask: 0,
        route_mask: 0,
        parallel_mask: 0,
        loop_mask: 0,
        passive_observer_mask: 0,
    };

    #[inline]
    pub(super) fn occupancy_mask(self) -> u8 {
        let len = self.len as usize;
        if len >= MAX_LANES {
            u8::MAX
        } else {
            (1u8 << len) - 1
        }
    }

    #[inline]
    pub(super) fn frontier_mask(self, frontier: FrontierKind) -> u8 {
        match frontier {
            FrontierKind::Route => self.route_mask,
            FrontierKind::Parallel => self.parallel_mask,
            FrontierKind::Loop => self.loop_mask,
            FrontierKind::PassiveObserver => self.passive_observer_mask,
        }
    }

    pub(super) fn insert_entry(&mut self, entry_idx: usize) -> Option<(u8, bool)> {
        if entry_idx >= MAX_STATES {
            return None;
        }
        let entry = checked_state_index(entry_idx)?;
        let observed_idx = self.slot_by_entry[entry_idx] as usize;
        if observed_idx < self.len as usize && self.entries[observed_idx] == entry {
            return Some((1u8 << observed_idx, false));
        }
        let observed_idx = self.len as usize;
        if observed_idx >= MAX_LANES {
            return None;
        }
        self.entries[observed_idx] = entry;
        self.slot_by_entry[entry_idx] = observed_idx as u8;
        self.len += 1;
        Some((1u8 << observed_idx, true))
    }

    #[inline]
    pub(super) fn entry_bit(self, entry_idx: usize) -> u8 {
        if entry_idx >= MAX_STATES {
            return 0;
        }
        let observed_idx = self.slot_by_entry[entry_idx] as usize;
        if observed_idx >= self.len as usize {
            return 0;
        }
        1u8 << observed_idx
    }

    #[inline]
    pub(super) fn first_entry_idx(self, mask: u8) -> Option<usize> {
        if mask == 0 {
            return None;
        }
        let observed_idx = mask.trailing_zeros() as usize;
        if observed_idx >= self.len as usize {
            return None;
        }
        Some(state_index_to_usize(self.entries[observed_idx]))
    }

    #[inline]
    pub(super) fn observe(&mut self, observed_bit: u8, observed: OfferEntryObservedState) {
        if observed.is_controller() {
            self.controller_mask |= observed_bit;
        }
        if observed.is_dynamic() {
            self.dynamic_controller_mask |= observed_bit;
        }
        if observed.has_progress_evidence() {
            self.progress_mask |= observed_bit;
        }
        if observed.has_ready_arm_evidence() {
            self.ready_arm_mask |= observed_bit;
        }
        if (observed.flags & OfferEntryObservedState::FLAG_READY) != 0 {
            self.ready_mask |= observed_bit;
        }
        if observed.matches_frontier(FrontierKind::Route) {
            self.route_mask |= observed_bit;
        }
        if observed.matches_frontier(FrontierKind::Parallel) {
            self.parallel_mask |= observed_bit;
        }
        if observed.matches_frontier(FrontierKind::Loop) {
            self.loop_mask |= observed_bit;
        }
        if observed.matches_frontier(FrontierKind::PassiveObserver) {
            self.passive_observer_mask |= observed_bit;
        }
    }

    #[inline]
    pub(super) fn replace_observation(
        &mut self,
        entry_idx: usize,
        observed: OfferEntryObservedState,
    ) -> bool {
        let observed_bit = self.entry_bit(entry_idx);
        if observed_bit == 0 {
            return false;
        }
        self.controller_mask &= !observed_bit;
        self.dynamic_controller_mask &= !observed_bit;
        self.progress_mask &= !observed_bit;
        self.ready_arm_mask &= !observed_bit;
        self.ready_mask &= !observed_bit;
        self.route_mask &= !observed_bit;
        self.parallel_mask &= !observed_bit;
        self.loop_mask &= !observed_bit;
        self.passive_observer_mask &= !observed_bit;
        self.observe(observed_bit, observed);
        true
    }

    pub(super) fn move_entry_slot(&mut self, entry_idx: usize, new_slot_idx: usize) -> bool {
        if entry_idx >= MAX_STATES {
            return false;
        }
        let old_slot_idx = self.slot_by_entry[entry_idx] as usize;
        let len = self.len as usize;
        if old_slot_idx >= len || new_slot_idx >= len {
            return false;
        }
        if old_slot_idx == new_slot_idx {
            return true;
        }
        let entry = self.entries[old_slot_idx];
        if old_slot_idx < new_slot_idx {
            let mut slot_idx = old_slot_idx;
            while slot_idx < new_slot_idx {
                self.entries[slot_idx] = self.entries[slot_idx + 1];
                self.slot_by_entry[state_index_to_usize(self.entries[slot_idx])] = slot_idx as u8;
                slot_idx += 1;
            }
        } else {
            let mut slot_idx = old_slot_idx;
            while slot_idx > new_slot_idx {
                self.entries[slot_idx] = self.entries[slot_idx - 1];
                self.slot_by_entry[state_index_to_usize(self.entries[slot_idx])] = slot_idx as u8;
                slot_idx -= 1;
            }
        }
        self.entries[new_slot_idx] = entry;
        self.slot_by_entry[entry_idx] = new_slot_idx as u8;
        self.controller_mask =
            Self::move_slot_mask(self.controller_mask, len, old_slot_idx, new_slot_idx);
        self.dynamic_controller_mask = Self::move_slot_mask(
            self.dynamic_controller_mask,
            len,
            old_slot_idx,
            new_slot_idx,
        );
        self.progress_mask =
            Self::move_slot_mask(self.progress_mask, len, old_slot_idx, new_slot_idx);
        self.ready_arm_mask =
            Self::move_slot_mask(self.ready_arm_mask, len, old_slot_idx, new_slot_idx);
        self.ready_mask = Self::move_slot_mask(self.ready_mask, len, old_slot_idx, new_slot_idx);
        self.route_mask = Self::move_slot_mask(self.route_mask, len, old_slot_idx, new_slot_idx);
        self.parallel_mask =
            Self::move_slot_mask(self.parallel_mask, len, old_slot_idx, new_slot_idx);
        self.loop_mask = Self::move_slot_mask(self.loop_mask, len, old_slot_idx, new_slot_idx);
        self.passive_observer_mask =
            Self::move_slot_mask(self.passive_observer_mask, len, old_slot_idx, new_slot_idx);
        true
    }

    pub(super) fn insert_observation_at_slot(
        &mut self,
        entry_idx: usize,
        slot_idx: usize,
        observed: OfferEntryObservedState,
    ) -> bool {
        if entry_idx >= MAX_STATES {
            return false;
        }
        let len = self.len as usize;
        if len >= MAX_LANES || slot_idx > len {
            return false;
        }
        let Some(entry) = checked_state_index(entry_idx) else {
            return false;
        };
        let existing_slot = self.slot_by_entry[entry_idx] as usize;
        if existing_slot < len && self.entries[existing_slot] == entry {
            return false;
        }
        let mut shift_idx = len;
        while shift_idx > slot_idx {
            self.entries[shift_idx] = self.entries[shift_idx - 1];
            self.slot_by_entry[state_index_to_usize(self.entries[shift_idx])] = shift_idx as u8;
            shift_idx -= 1;
        }
        self.entries[slot_idx] = entry;
        self.slot_by_entry[entry_idx] = slot_idx as u8;
        self.len += 1;
        self.controller_mask = Self::insert_slot_mask(self.controller_mask, len, slot_idx);
        self.dynamic_controller_mask =
            Self::insert_slot_mask(self.dynamic_controller_mask, len, slot_idx);
        self.progress_mask = Self::insert_slot_mask(self.progress_mask, len, slot_idx);
        self.ready_arm_mask = Self::insert_slot_mask(self.ready_arm_mask, len, slot_idx);
        self.ready_mask = Self::insert_slot_mask(self.ready_mask, len, slot_idx);
        self.route_mask = Self::insert_slot_mask(self.route_mask, len, slot_idx);
        self.parallel_mask = Self::insert_slot_mask(self.parallel_mask, len, slot_idx);
        self.loop_mask = Self::insert_slot_mask(self.loop_mask, len, slot_idx);
        self.passive_observer_mask =
            Self::insert_slot_mask(self.passive_observer_mask, len, slot_idx);
        self.observe(1u8 << slot_idx, observed);
        true
    }

    pub(super) fn remove_observation(&mut self, entry_idx: usize) -> bool {
        if entry_idx >= MAX_STATES {
            return false;
        }
        let slot_idx = self.slot_by_entry[entry_idx] as usize;
        let len = self.len as usize;
        if slot_idx >= len {
            return false;
        }
        let Some(entry) = checked_state_index(entry_idx) else {
            return false;
        };
        if self.entries[slot_idx] != entry {
            return false;
        }
        let mut shift_idx = slot_idx;
        while shift_idx + 1 < len {
            self.entries[shift_idx] = self.entries[shift_idx + 1];
            self.slot_by_entry[state_index_to_usize(self.entries[shift_idx])] = shift_idx as u8;
            shift_idx += 1;
        }
        self.entries[len - 1] = StateIndex::MAX;
        self.slot_by_entry[entry_idx] = u8::MAX;
        self.len = self.len.saturating_sub(1);
        self.controller_mask = Self::remove_slot_mask(self.controller_mask, len, slot_idx);
        self.dynamic_controller_mask =
            Self::remove_slot_mask(self.dynamic_controller_mask, len, slot_idx);
        self.progress_mask = Self::remove_slot_mask(self.progress_mask, len, slot_idx);
        self.ready_arm_mask = Self::remove_slot_mask(self.ready_arm_mask, len, slot_idx);
        self.ready_mask = Self::remove_slot_mask(self.ready_mask, len, slot_idx);
        self.route_mask = Self::remove_slot_mask(self.route_mask, len, slot_idx);
        self.parallel_mask = Self::remove_slot_mask(self.parallel_mask, len, slot_idx);
        self.loop_mask = Self::remove_slot_mask(self.loop_mask, len, slot_idx);
        self.passive_observer_mask =
            Self::remove_slot_mask(self.passive_observer_mask, len, slot_idx);
        true
    }

    pub(super) fn replace_entry_at_slot(
        &mut self,
        old_entry_idx: usize,
        new_entry_idx: usize,
        observed: OfferEntryObservedState,
    ) -> bool {
        if old_entry_idx >= MAX_STATES || new_entry_idx >= MAX_STATES {
            return false;
        }
        let slot_idx = self.slot_by_entry[old_entry_idx] as usize;
        let len = self.len as usize;
        if slot_idx >= len {
            return false;
        }
        let Some(old_entry) = checked_state_index(old_entry_idx) else {
            return false;
        };
        let Some(new_entry) = checked_state_index(new_entry_idx) else {
            return false;
        };
        if self.entries[slot_idx] != old_entry {
            return false;
        }
        let existing_new_slot = self.slot_by_entry[new_entry_idx] as usize;
        if existing_new_slot < len {
            return false;
        }
        let observed_bit = 1u8 << slot_idx;
        self.entries[slot_idx] = new_entry;
        self.slot_by_entry[old_entry_idx] = u8::MAX;
        self.slot_by_entry[new_entry_idx] = slot_idx as u8;
        self.controller_mask &= !observed_bit;
        self.dynamic_controller_mask &= !observed_bit;
        self.progress_mask &= !observed_bit;
        self.ready_arm_mask &= !observed_bit;
        self.ready_mask &= !observed_bit;
        self.route_mask &= !observed_bit;
        self.parallel_mask &= !observed_bit;
        self.loop_mask &= !observed_bit;
        self.passive_observer_mask &= !observed_bit;
        self.observe(observed_bit, observed);
        true
    }

    pub(super) fn move_slot_mask(
        mask: u8,
        len: usize,
        old_slot_idx: usize,
        new_slot_idx: usize,
    ) -> u8 {
        let mut remapped = 0u8;
        let mut slot_idx = 0usize;
        while slot_idx < len {
            let source_slot = if old_slot_idx < new_slot_idx {
                if slot_idx < old_slot_idx || slot_idx > new_slot_idx {
                    slot_idx
                } else if slot_idx == new_slot_idx {
                    old_slot_idx
                } else {
                    slot_idx + 1
                }
            } else if slot_idx < new_slot_idx || slot_idx > old_slot_idx {
                slot_idx
            } else if slot_idx == new_slot_idx {
                old_slot_idx
            } else {
                slot_idx - 1
            };
            if ((mask >> source_slot) & 1) != 0 {
                remapped |= 1u8 << slot_idx;
            }
            slot_idx += 1;
        }
        remapped
    }

    pub(super) fn insert_slot_mask(mask: u8, len: usize, slot_idx: usize) -> u8 {
        let mut remapped = 0u8;
        let mut new_slot_idx = 0usize;
        while new_slot_idx <= len {
            if new_slot_idx == slot_idx {
                new_slot_idx += 1;
                continue;
            }
            let source_slot = if new_slot_idx < slot_idx {
                new_slot_idx
            } else {
                new_slot_idx - 1
            };
            if source_slot < len && ((mask >> source_slot) & 1) != 0 {
                remapped |= 1u8 << new_slot_idx;
            }
            new_slot_idx += 1;
        }
        remapped
    }

    pub(super) fn remove_slot_mask(mask: u8, len: usize, slot_idx: usize) -> u8 {
        if len == 0 || slot_idx >= len {
            return 0;
        }
        let mut remapped = 0u8;
        let mut new_slot_idx = 0usize;
        while new_slot_idx + 1 < len {
            let source_slot = if new_slot_idx < slot_idx {
                new_slot_idx
            } else {
                new_slot_idx + 1
            };
            if ((mask >> source_slot) & 1) != 0 {
                remapped |= 1u8 << new_slot_idx;
            }
            new_slot_idx += 1;
        }
        remapped
    }
}

#[derive(Clone, Copy)]
pub(super) struct RootFrontierState {
    pub(super) root: ScopeId,
    pub(super) active_mask: u8,
    pub(super) controller_mask: u8,
    pub(super) dynamic_controller_mask: u8,
    pub(super) offer_lane_mask: u8,
    pub(super) offer_lane_entry_slot_masks: [u8; MAX_LANES],
    pub(super) observed_epoch: u32,
    pub(super) observed_key: FrontierObservationKey,
    pub(super) active_entries: ActiveEntrySet,
    pub(super) observed_entries: ObservedEntrySet,
}

impl RootFrontierState {
    pub(super) const EMPTY: Self = Self {
        root: ScopeId::none(),
        active_mask: 0,
        controller_mask: 0,
        dynamic_controller_mask: 0,
        offer_lane_mask: 0,
        offer_lane_entry_slot_masks: [0; MAX_LANES],
        observed_epoch: 0,
        observed_key: FrontierObservationKey::EMPTY,
        active_entries: ActiveEntrySet::EMPTY,
        observed_entries: ObservedEntrySet::EMPTY,
    };
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct FrontierObservationKey {
    pub(super) active_entries: [StateIndex; MAX_LANES],
    pub(super) entry_summary_fingerprints: [u8; MAX_LANES],
    pub(super) scope_generations: [u32; MAX_LANES],
    pub(super) offer_lane_mask: u8,
    pub(super) binding_nonempty_mask: u8,
    pub(super) route_change_epochs: [u32; MAX_LANES],
}

impl FrontierObservationKey {
    pub(super) const EMPTY: Self = Self {
        active_entries: [StateIndex::MAX; MAX_LANES],
        entry_summary_fingerprints: [0; MAX_LANES],
        scope_generations: [0; MAX_LANES],
        offer_lane_mask: 0,
        binding_nonempty_mask: 0,
        route_change_epochs: [0; MAX_LANES],
    };
}

#[derive(Clone, Copy)]
pub(super) struct OfferEntryStaticSummary {
    pub(super) frontier_mask: u8,
    pub(super) flags: u8,
}

impl OfferEntryStaticSummary {
    pub(super) const FLAG_CONTROLLER: u8 = 1;
    pub(super) const FLAG_DYNAMIC: u8 = 1 << 1;
    pub(super) const FLAG_STATIC_READY: u8 = 1 << 2;

    pub(super) const EMPTY: Self = Self {
        frontier_mask: 0,
        flags: 0,
    };

    #[inline]
    pub(super) fn observe_lane(&mut self, info: LaneOfferState) {
        self.frontier_mask |= info.frontier.bit();
        if info.is_controller() {
            self.flags |= Self::FLAG_CONTROLLER;
        }
        if info.is_dynamic() {
            self.flags |= Self::FLAG_DYNAMIC;
        }
        if info.static_ready() {
            self.flags |= Self::FLAG_STATIC_READY;
        }
    }

    #[inline]
    pub(super) fn is_controller(self) -> bool {
        (self.flags & Self::FLAG_CONTROLLER) != 0
    }

    #[inline]
    pub(super) fn is_dynamic(self) -> bool {
        (self.flags & Self::FLAG_DYNAMIC) != 0
    }

    #[inline]
    pub(super) fn static_ready(self) -> bool {
        (self.flags & Self::FLAG_STATIC_READY) != 0
    }

    #[inline]
    pub(super) fn observation_fingerprint(self) -> u8 {
        self.frontier_mask | (self.flags << 4)
    }
}

#[derive(Clone, Copy)]
pub(super) struct OfferEntryState {
    pub(super) active_mask: u8,
    pub(super) lane_idx: u8,
    pub(super) parallel_root: ScopeId,
    pub(super) frontier: FrontierKind,
    pub(super) scope_id: ScopeId,
    pub(super) offer_lane_mask: u8,
    pub(super) offer_lanes: [u8; MAX_LANES],
    pub(super) offer_lanes_len: u8,
    pub(super) selection_meta: CurrentScopeSelectionMeta,
    pub(super) label_meta: ScopeLabelMeta,
    pub(super) materialization_meta: ScopeArmMaterializationMeta,
    pub(super) summary: OfferEntryStaticSummary,
    pub(super) observed: OfferEntryObservedState,
}

impl OfferEntryState {
    pub(super) const EMPTY: Self = Self {
        active_mask: 0,
        lane_idx: u8::MAX,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::Route,
        scope_id: ScopeId::none(),
        offer_lane_mask: 0,
        offer_lanes: [0; MAX_LANES],
        offer_lanes_len: 0,
        selection_meta: CurrentScopeSelectionMeta::EMPTY,
        label_meta: ScopeLabelMeta::EMPTY,
        materialization_meta: ScopeArmMaterializationMeta::EMPTY,
        summary: OfferEntryStaticSummary::EMPTY,
        observed: OfferEntryObservedState::EMPTY,
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct OfferEntryObservedState {
    pub(super) scope_id: ScopeId,
    pub(super) frontier_mask: u8,
    pub(super) flags: u8,
}

impl OfferEntryObservedState {
    pub(super) const EMPTY: Self = Self {
        scope_id: ScopeId::none(),
        frontier_mask: 0,
        flags: 0,
    };
    pub(super) const FLAG_CONTROLLER: u8 = 1;
    pub(super) const FLAG_DYNAMIC: u8 = 1 << 1;
    pub(super) const FLAG_PROGRESS: u8 = 1 << 2;
    pub(super) const FLAG_READY_ARM: u8 = 1 << 3;
    pub(super) const FLAG_BINDING_READY: u8 = 1 << 4;
    pub(super) const FLAG_READY: u8 = 1 << 5;

    #[inline]
    pub(super) fn is_controller(self) -> bool {
        (self.flags & Self::FLAG_CONTROLLER) != 0
    }

    #[inline]
    pub(super) fn is_dynamic(self) -> bool {
        (self.flags & Self::FLAG_DYNAMIC) != 0
    }

    #[inline]
    pub(super) fn has_progress_evidence(self) -> bool {
        (self.flags & Self::FLAG_PROGRESS) != 0
    }

    #[inline]
    pub(super) fn has_ready_arm_evidence(self) -> bool {
        (self.flags & Self::FLAG_READY_ARM) != 0
    }

    #[inline]
    pub(super) fn ready(self) -> bool {
        (self.flags & Self::FLAG_READY) != 0
    }

    #[cfg(test)]
    #[inline]
    pub(super) fn binding_ready(self) -> bool {
        (self.flags & Self::FLAG_BINDING_READY) != 0
    }

    #[inline]
    pub(super) fn matches_frontier(self, frontier: FrontierKind) -> bool {
        (self.frontier_mask & frontier.bit()) != 0
    }
}

pub(super) const MAX_ROUTE_ARM_STACK: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct FrontierCandidate {
    pub(super) scope_id: ScopeId,
    pub(super) entry_idx: usize,
    pub(super) parallel_root: ScopeId,
    pub(super) frontier: FrontierKind,
    pub(super) is_controller: bool,
    pub(super) is_dynamic: bool,
    pub(super) has_evidence: bool,
    pub(super) ready: bool,
}

impl FrontierCandidate {
    pub(super) const EMPTY: Self = Self {
        scope_id: ScopeId::none(),
        entry_idx: usize::MAX,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::Route,
        is_controller: false,
        is_dynamic: false,
        has_evidence: false,
        ready: false,
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct FrontierSnapshot {
    pub(super) current_scope: ScopeId,
    pub(super) current_entry_idx: usize,
    pub(super) current_parallel_root: ScopeId,
    pub(super) current_frontier: FrontierKind,
    pub(super) candidates: [FrontierCandidate; MAX_LANES],
    pub(super) candidate_len: usize,
}

impl FrontierSnapshot {
    #[inline]
    pub(super) fn matches_parallel_root(self, candidate: FrontierCandidate) -> bool {
        self.current_parallel_root.is_none()
            || candidate.parallel_root == self.current_parallel_root
    }

    pub(super) fn select_yield_candidate(
        self,
        visited: FrontierVisitSet,
    ) -> Option<FrontierCandidate> {
        let mut idx = 0usize;
        while idx < self.candidate_len {
            let candidate = self.candidates[idx];
            if (candidate.scope_id != self.current_scope
                || candidate.entry_idx != self.current_entry_idx)
                && self.matches_parallel_root(candidate)
                && candidate.frontier == self.current_frontier
                && candidate.ready
                && candidate.has_evidence
                && !visited.contains(candidate.scope_id)
            {
                return Some(candidate);
            }
            idx += 1;
        }
        idx = 0;
        while idx < self.candidate_len {
            let candidate = self.candidates[idx];
            if (candidate.scope_id != self.current_scope
                || candidate.entry_idx != self.current_entry_idx)
                && self.matches_parallel_root(candidate)
                && candidate.ready
                && candidate.has_evidence
                && !visited.contains(candidate.scope_id)
            {
                return Some(candidate);
            }
            idx += 1;
        }
        None
    }

    pub(super) fn select_exhausted_controller_candidate(
        self,
        visited: FrontierVisitSet,
    ) -> Option<FrontierCandidate> {
        let mut idx = 0usize;
        while idx < self.candidate_len {
            let candidate = self.candidates[idx];
            if (candidate.scope_id != self.current_scope
                || candidate.entry_idx != self.current_entry_idx)
                && self.matches_parallel_root(candidate)
                && candidate.frontier == self.current_frontier
                && candidate.is_controller
                && candidate.ready
                && candidate.has_evidence
                && !visited.contains(candidate.scope_id)
            {
                return Some(candidate);
            }
            idx += 1;
        }
        idx = 0;
        while idx < self.candidate_len {
            let candidate = self.candidates[idx];
            if (candidate.scope_id != self.current_scope
                || candidate.entry_idx != self.current_entry_idx)
                && self.matches_parallel_root(candidate)
                && candidate.is_controller
                && candidate.ready
                && candidate.has_evidence
                && !visited.contains(candidate.scope_id)
            {
                return Some(candidate);
            }
            idx += 1;
        }
        None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct FrontierVisitSet {
    pub(super) slots: [ScopeId; MAX_LANES],
    pub(super) len: usize,
}

impl FrontierVisitSet {
    pub(super) const EMPTY: Self = Self {
        slots: [ScopeId::none(); MAX_LANES],
        len: 0,
    };

    #[inline]
    pub(super) fn contains(self, scope: ScopeId) -> bool {
        let mut idx = 0usize;
        while idx < self.len {
            if self.slots[idx] == scope {
                return true;
            }
            idx += 1;
        }
        false
    }

    #[inline]
    pub(super) fn record(&mut self, scope: ScopeId) {
        if scope.is_none() || self.contains(scope) || self.len >= MAX_LANES {
            return;
        }
        self.slots[self.len] = scope;
        self.len += 1;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FrontierDeferOutcome {
    Continue,
    Yielded,
    Exhausted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct EvidenceFingerprint(u8);

impl EvidenceFingerprint {
    #[inline]
    pub(super) const fn new(
        has_ack: bool,
        has_ready_arm_evidence: bool,
        binding_ready: bool,
    ) -> Self {
        let mut bits = 0u8;
        if has_ack {
            bits |= 1 << 0;
        }
        if has_ready_arm_evidence {
            bits |= 1 << 1;
        }
        if binding_ready {
            bits |= 1 << 2;
        }
        Self(bits)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct OfferLivenessState {
    pub(super) policy: crate::runtime::config::LivenessPolicy,
    pub(super) remaining_defer: u8,
    pub(super) remaining_no_evidence_defer: u8,
    pub(super) forced_poll_attempts: u8,
    pub(super) last_fingerprint: Option<EvidenceFingerprint>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DeferBudgetOutcome {
    Continue,
    Exhausted,
}

impl OfferLivenessState {
    #[inline]
    pub(super) fn new(policy: crate::runtime::config::LivenessPolicy) -> Self {
        Self {
            policy,
            remaining_defer: policy.max_defer_per_offer,
            remaining_no_evidence_defer: policy.max_no_evidence_defer,
            forced_poll_attempts: 0,
            last_fingerprint: None,
        }
    }

    #[inline]
    pub(super) fn on_defer(&mut self, fingerprint: EvidenceFingerprint) -> DeferBudgetOutcome {
        if self.remaining_defer == 0 {
            return DeferBudgetOutcome::Exhausted;
        }
        self.remaining_defer = self.remaining_defer.saturating_sub(1);
        let has_new_evidence = self.last_fingerprint != Some(fingerprint);
        self.last_fingerprint = Some(fingerprint);
        if !has_new_evidence {
            if self.remaining_no_evidence_defer == 0 {
                return DeferBudgetOutcome::Exhausted;
            }
            self.remaining_no_evidence_defer = self.remaining_no_evidence_defer.saturating_sub(1);
        }
        DeferBudgetOutcome::Continue
    }

    #[inline]
    pub(super) const fn can_force_poll(self) -> bool {
        self.policy.force_poll_on_exhaustion
            && self.forced_poll_attempts < self.policy.max_forced_poll_attempts
    }

    #[inline]
    pub(super) fn mark_forced_poll(&mut self) {
        self.forced_poll_attempts = self.forced_poll_attempts.saturating_add(1);
    }

    #[inline]
    pub(super) const fn exhaust_reason(self) -> u16 {
        self.policy.exhaust_reason
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum OfferSelectPriority {
    CurrentOfferEntry,
    DynamicControllerUnique,
    ControllerUnique,
    CandidateUnique,
}

#[inline]
pub(super) fn choose_offer_priority(
    current_is_candidate: bool,
    dynamic_controller_count: usize,
    controller_count: usize,
    candidate_count: usize,
) -> Option<OfferSelectPriority> {
    if current_is_candidate {
        Some(OfferSelectPriority::CurrentOfferEntry)
    } else if dynamic_controller_count == 1 {
        Some(OfferSelectPriority::DynamicControllerUnique)
    } else if controller_count == 1 {
        Some(OfferSelectPriority::ControllerUnique)
    } else if candidate_count == 1 {
        Some(OfferSelectPriority::CandidateUnique)
    } else {
        None
    }
}

#[inline]
pub(super) async fn yield_once() {
    let mut yielded = false;
    poll_fn(|cx| {
        if yielded {
            Poll::Ready(())
        } else {
            yielded = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    })
    .await
}

#[inline]
pub(super) fn current_entry_is_candidate(
    current_matches_candidate: bool,
    current_is_controller: bool,
    current_has_evidence: bool,
    candidate_count: usize,
    progress_sibling_exists: bool,
) -> bool {
    if !current_matches_candidate {
        return false;
    }
    if current_is_controller
        && !current_has_evidence
        && progress_sibling_exists
        && candidate_count > 0
    {
        return false;
    }
    true
}

#[inline]
pub(super) fn current_entry_matches_after_filter(
    current_matches_candidate: bool,
    current_has_offer_lanes: bool,
    current_idx: usize,
    hint_filter: Option<usize>,
) -> bool {
    if !current_matches_candidate || !current_has_offer_lanes {
        return false;
    }
    if let Some(filtered_idx) = hint_filter {
        return current_idx == filtered_idx;
    }
    true
}

#[inline]
pub(super) fn should_suppress_current_passive_without_evidence(
    current_frontier: FrontierKind,
    current_is_controller: bool,
    current_has_evidence: bool,
    controller_progress_sibling_exists: bool,
) -> bool {
    current_frontier == FrontierKind::PassiveObserver
        && !current_is_controller
        && !current_has_evidence
        && controller_progress_sibling_exists
}

#[cfg(test)]
#[inline]
pub(super) fn candidate_participates_in_frontier_arbitration(
    entry_idx: usize,
    current_idx: usize,
    has_progress_evidence: bool,
    current_entry_unrunnable: bool,
) -> bool {
    entry_idx == current_idx
        || has_progress_evidence
        || (current_entry_unrunnable && entry_idx != current_idx)
}

#[cfg(test)]
#[inline]
pub(super) fn controller_candidate_ready(
    is_controller: bool,
    entry_idx: usize,
    current_idx: usize,
    has_progress_evidence: bool,
) -> bool {
    !is_controller || entry_idx == current_idx || has_progress_evidence
}

#[inline]
pub(super) fn candidate_has_progress_evidence(
    has_ready_arm_evidence: bool,
    ack_is_progress: bool,
    binding_ready: bool,
) -> bool {
    has_ready_arm_evidence || ack_is_progress || binding_ready
}

#[inline]
pub(super) fn offer_entry_observed_state(
    scope_id: ScopeId,
    summary: OfferEntryStaticSummary,
    has_ready_arm_evidence: bool,
    ack_is_progress: bool,
    binding_ready: bool,
) -> OfferEntryObservedState {
    let has_progress_evidence =
        candidate_has_progress_evidence(has_ready_arm_evidence, ack_is_progress, binding_ready);
    let ready =
        has_ready_arm_evidence || ack_is_progress || binding_ready || summary.static_ready();
    let mut flags = 0u8;
    if summary.is_controller() {
        flags |= OfferEntryObservedState::FLAG_CONTROLLER;
    }
    if summary.is_dynamic() {
        flags |= OfferEntryObservedState::FLAG_DYNAMIC;
    }
    if has_progress_evidence {
        flags |= OfferEntryObservedState::FLAG_PROGRESS;
    }
    if has_ready_arm_evidence {
        flags |= OfferEntryObservedState::FLAG_READY_ARM;
    }
    if binding_ready {
        flags |= OfferEntryObservedState::FLAG_BINDING_READY;
    }
    if ready {
        flags |= OfferEntryObservedState::FLAG_READY;
    }
    OfferEntryObservedState {
        scope_id,
        frontier_mask: summary.frontier_mask,
        flags,
    }
}

#[inline]
pub(super) fn offer_entry_frontier_candidate(
    entry_idx: usize,
    parallel_root: ScopeId,
    frontier: FrontierKind,
    observed: OfferEntryObservedState,
) -> FrontierCandidate {
    FrontierCandidate {
        scope_id: observed.scope_id,
        entry_idx,
        parallel_root,
        frontier,
        is_controller: observed.is_controller(),
        is_dynamic: observed.is_dynamic(),
        has_evidence: observed.has_progress_evidence(),
        ready: observed.ready(),
    }
}

#[inline]
pub(super) fn cached_offer_entry_observed_state(
    scope_id: ScopeId,
    summary: OfferEntryStaticSummary,
    observed_entries: ObservedEntrySet,
    observed_bit: u8,
) -> OfferEntryObservedState {
    let mut flags = 0u8;
    if summary.is_controller() {
        flags |= OfferEntryObservedState::FLAG_CONTROLLER;
    }
    if summary.is_dynamic() {
        flags |= OfferEntryObservedState::FLAG_DYNAMIC;
    }
    if (observed_entries.progress_mask & observed_bit) != 0 {
        flags |= OfferEntryObservedState::FLAG_PROGRESS;
    }
    if (observed_entries.ready_arm_mask & observed_bit) != 0 {
        flags |= OfferEntryObservedState::FLAG_READY_ARM;
    }
    if (observed_entries.ready_mask & observed_bit) != 0 {
        flags |= OfferEntryObservedState::FLAG_READY;
    }
    OfferEntryObservedState {
        scope_id,
        frontier_mask: summary.frontier_mask,
        flags,
    }
}

#[cfg(test)]
#[inline]
pub(super) fn record_offer_entry_reentry_candidate(
    current_scope: ScopeId,
    current_entry_idx: usize,
    current_parallel_root: ScopeId,
    candidate: FrontierCandidate,
    ready_entry_idx: &mut Option<usize>,
    any_entry_idx: &mut Option<usize>,
) {
    if (candidate.scope_id == current_scope && candidate.entry_idx == current_entry_idx)
        || (!current_parallel_root.is_none() && candidate.parallel_root != current_parallel_root)
    {
        return;
    }
    if any_entry_idx.is_none() {
        *any_entry_idx = Some(candidate.entry_idx);
    }
    if candidate.ready && ready_entry_idx.is_none() {
        *ready_entry_idx = Some(candidate.entry_idx);
    }
}
