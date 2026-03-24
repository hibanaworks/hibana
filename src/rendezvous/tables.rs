//! Internal state tables for ra module.
//!
//! These tables manage generation counters, fences, acks, checkpoints, and routing policies.
//! All tables are !Send/!Sync (single-threaded, no_std compatible).

use core::{
    array,
    cell::UnsafeCell,
    marker::PhantomData,
    task::{Context, Poll, Waker},
};

use super::error::{GenError, GenerationRecord};
use crate::{
    control::{
        cap::mint::CapsMask,
        lease::map::ArrayMap,
        types::{Generation, Lane},
    },
    eff::{self, EffIndex},
    global::const_dsl::{PolicyMode, ScopeId, ScopeKind},
    runtime::consts::LANES_MAX,
};

const ROLE_SLOTS: usize = LANES_MAX as usize;
const LOOP_SLOTS: usize = eff::meta::MAX_EFF_NODES;
const ROUTE_SLOTS: usize = eff::meta::MAX_EFF_NODES;
const ROUTE_SCOPE_ORDINAL_CAPACITY: usize = ScopeId::ORDINAL_CAPACITY as usize;
const ROUTE_PENDING_HINT_LABEL_CAPACITY: usize = u128::BITS as usize;
const ROUTE_SLOT_INDEX_NONE: u16 = u16::MAX;
const CONTROL_PLAN_SLOTS: usize = 128;

/// Generation counter table (per-lane).
///
/// Tracks the last seen generation number for each lane to ensure monotonic updates.
pub(crate) struct GenTable {
    lanes: UnsafeCell<[Option<u16>; LANES_MAX as usize]>,
    _no_send_sync: PhantomData<*mut ()>,
}

impl Default for GenTable {
    fn default() -> Self {
        Self::new()
    }
}

impl GenTable {
    pub(crate) const fn new() -> Self {
        Self {
            lanes: UnsafeCell::new([None; LANES_MAX as usize]),
            _no_send_sync: PhantomData,
        }
    }

    /// Check and update generation for a lane.
    ///
    /// # Safety
    /// Rendezvous/Port are !Send/!Sync; writer is single-producer.
    #[inline]
    pub(crate) fn check_and_update(&self, lane: Lane, new: Generation) -> Result<(), GenError> {
        unsafe {
            let buf = &mut *self.lanes.get();
            let idx = lane.raw() as usize;
            match buf[idx] {
                None => {
                    if new.raw() == 0 {
                        buf[idx] = Some(new.raw());
                        Ok(())
                    } else {
                        Err(GenError::InvalidInitial { lane, new })
                    }
                }
                Some(prev) if prev == u16::MAX => Err(GenError::Overflow {
                    lane,
                    last: Generation::new(prev),
                }),
                Some(prev) if new.raw() > prev => {
                    buf[idx] = Some(new.raw());
                    Ok(())
                }
                Some(prev) => Err(GenError::StaleOrDuplicate(GenerationRecord {
                    lane,
                    last: Generation::new(prev),
                    new,
                })),
            }
        }
    }

    /// Get last generation for a lane.
    #[inline]
    pub(crate) fn last(&self, lane: Lane) -> Option<Generation> {
        unsafe {
            let buf = &*self.lanes.get();
            buf[lane.raw() as usize].map(Generation::new)
        }
    }

    /// Reset lane (for release).
    #[inline]
    pub(crate) fn reset_lane(&self, lane: Lane) {
        unsafe {
            (*self.lanes.get())[lane.raw() as usize] = None;
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LoopDisposition {
    Continue,
    Break,
}

#[derive(Clone, Copy)]
struct LoopEntry {
    epoch: u16,
    decision: LoopDisposition,
    last_seen: [u16; ROLE_SLOTS],
}

impl LoopEntry {
    const fn empty() -> Self {
        Self {
            epoch: 0,
            decision: LoopDisposition::Break,
            last_seen: [0; ROLE_SLOTS],
        }
    }
}

fn init_waiters() -> [[Option<Waker>; ROLE_SLOTS]; LANES_MAX as usize] {
    array::from_fn(|_| array::from_fn(|_| None))
}

pub(crate) struct LoopTable {
    entries: UnsafeCell<[[LoopEntry; LOOP_SLOTS]; LANES_MAX as usize]>,
    waiters: UnsafeCell<[[Option<Waker>; ROLE_SLOTS]; LANES_MAX as usize]>,
    _no_send_sync: PhantomData<*mut ()>,
}

impl Default for LoopTable {
    fn default() -> Self {
        Self::new()
    }
}

impl LoopTable {
    pub(crate) fn new() -> Self {
        Self {
            entries: UnsafeCell::new([[LoopEntry::empty(); LOOP_SLOTS]; LANES_MAX as usize]),
            waiters: UnsafeCell::new(init_waiters()),
            _no_send_sync: PhantomData,
        }
    }

    #[inline]
    fn lane_idx(lane: Lane) -> usize {
        lane.raw() as usize
    }

    #[inline]
    fn loop_idx(idx: u8) -> usize {
        idx as usize
    }

    pub(crate) fn record(
        &self,
        lane: Lane,
        role_from: u8,
        idx: u8,
        disposition: LoopDisposition,
    ) -> u16 {
        let entries = unsafe { &mut *self.entries.get() };
        let lane_idx = Self::lane_idx(lane);
        let slot_idx = Self::loop_idx(idx);
        let entry = &mut entries[lane_idx][slot_idx];
        let mut epoch = entry.epoch.wrapping_add(1);
        if epoch == 0 {
            epoch = 1;
        }
        entry.epoch = epoch;
        entry.decision = disposition;
        if (role_from as usize) < ROLE_SLOTS {
            entry.last_seen[role_from as usize] = epoch;
        }

        let waiters = unsafe { &mut *self.waiters.get() };
        let row = &mut waiters[lane_idx];
        for waiter in row.iter_mut() {
            if let Some(waker) = waiter.take() {
                waker.wake();
            }
        }
        epoch
    }

    pub(crate) fn acknowledge(&self, lane: Lane, role: u8, idx: u8) {
        if (role as usize) >= ROLE_SLOTS {
            return;
        }
        let entries = unsafe { &mut *self.entries.get() };
        let lane_idx = Self::lane_idx(lane);
        let slot_idx = Self::loop_idx(idx);
        let entry = &mut entries[lane_idx][slot_idx];
        let epoch = entry.epoch;
        if epoch != 0 {
            entry.last_seen[role as usize] = epoch;
        }
    }

    pub(crate) fn reset_lane(&self, lane: Lane) {
        let entries = unsafe { &mut *self.entries.get() };
        let lane_idx = Self::lane_idx(lane);
        entries[lane_idx] = [LoopEntry::empty(); LOOP_SLOTS];

        let waiters = unsafe { &mut *self.waiters.get() };
        let row = &mut waiters[lane_idx];
        for waiter in row.iter_mut() {
            *waiter = None;
        }
    }

    #[inline]
    pub(crate) fn has_decision(&self, lane: Lane, idx: u8) -> bool {
        let entries = unsafe { &*self.entries.get() };
        let lane_idx = Self::lane_idx(lane);
        let slot_idx = Self::loop_idx(idx);
        entries[lane_idx][slot_idx].epoch != 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ScopeCoord {
    canonical: ScopeId,
}

impl ScopeCoord {
    fn from_scope(scope: ScopeId) -> Option<Self> {
        if scope.is_none() || scope.kind() != ScopeKind::Route {
            return None;
        }
        Some(Self {
            canonical: scope.canonical(),
        })
    }
}

#[derive(Clone, Copy)]
struct RouteEntry {
    epoch: u16,
    arm: u8,
    last_seen: [u16; ROLE_SLOTS],
}

impl RouteEntry {
    const fn empty() -> Self {
        Self {
            epoch: 0,
            arm: 0,
            last_seen: [0; ROLE_SLOTS],
        }
    }
}

#[derive(Clone, Copy)]
struct RouteSlot {
    coord: Option<ScopeCoord>,
    entry: RouteEntry,
}

impl RouteSlot {
    const fn empty() -> Self {
        Self {
            coord: None,
            entry: RouteEntry::empty(),
        }
    }

    fn assign(coord: ScopeCoord) -> Self {
        Self {
            coord: Some(coord),
            entry: RouteEntry::empty(),
        }
    }
}

#[derive(Clone, Copy)]
struct RoutePendingMaskSlot {
    coord: Option<ScopeCoord>,
    lane_masks: [u16; ROLE_SLOTS],
}

impl RoutePendingMaskSlot {
    const fn empty() -> Self {
        Self {
            coord: None,
            lane_masks: [0; ROLE_SLOTS],
        }
    }

    const fn assign(coord: ScopeCoord) -> Self {
        Self {
            coord: Some(coord),
            lane_masks: [0; ROLE_SLOTS],
        }
    }
}

pub(crate) struct RouteTable {
    slots: UnsafeCell<[[RouteSlot; ROUTE_SLOTS]; LANES_MAX as usize]>,
    slot_by_ordinal: UnsafeCell<[[u16; ROUTE_SCOPE_ORDINAL_CAPACITY]; LANES_MAX as usize]>,
    pending_mask_slots: UnsafeCell<[RoutePendingMaskSlot; ROUTE_SLOTS]>,
    pending_hint_lane_masks: UnsafeCell<[u16; ROUTE_PENDING_HINT_LABEL_CAPACITY]>,
    change_epoch: UnsafeCell<u32>,
    next_free: UnsafeCell<[u16; LANES_MAX as usize]>,
    waiters: UnsafeCell<[[Option<Waker>; ROLE_SLOTS]; LANES_MAX as usize]>,
    _no_send_sync: PhantomData<*mut ()>,
}

impl Default for RouteTable {
    fn default() -> Self {
        Self::new()
    }
}

impl RouteTable {
    pub(crate) fn new() -> Self {
        Self {
            slots: UnsafeCell::new([[RouteSlot::empty(); ROUTE_SLOTS]; LANES_MAX as usize]),
            slot_by_ordinal: UnsafeCell::new(
                [[ROUTE_SLOT_INDEX_NONE; ROUTE_SCOPE_ORDINAL_CAPACITY]; LANES_MAX as usize],
            ),
            pending_mask_slots: UnsafeCell::new([RoutePendingMaskSlot::empty(); ROUTE_SLOTS]),
            pending_hint_lane_masks: UnsafeCell::new([0; ROUTE_PENDING_HINT_LABEL_CAPACITY]),
            change_epoch: UnsafeCell::new(0),
            next_free: UnsafeCell::new([0; LANES_MAX as usize]),
            waiters: UnsafeCell::new(init_waiters()),
            _no_send_sync: PhantomData,
        }
    }

    #[inline]
    fn lane_idx(lane: Lane) -> usize {
        lane.raw() as usize
    }

    #[inline]
    fn scope_ordinal(coord: ScopeCoord) -> Option<usize> {
        let ordinal = coord.canonical.local_ordinal() as usize;
        if ordinal >= ROUTE_SCOPE_ORDINAL_CAPACITY {
            None
        } else {
            Some(ordinal)
        }
    }

    #[inline]
    fn slot_for_scope(&self, lane_idx: usize, coord: ScopeCoord) -> Option<usize> {
        let ordinal = Self::scope_ordinal(coord)?;
        let slot_map = unsafe { &*self.slot_by_ordinal.get() };
        let slot_idx = slot_map[lane_idx][ordinal];
        if slot_idx == ROUTE_SLOT_INDEX_NONE {
            None
        } else {
            Some(slot_idx as usize)
        }
    }

    fn slot_or_alloc(&self, lane_idx: usize, coord: ScopeCoord) -> Option<usize> {
        let ordinal = Self::scope_ordinal(coord)?;
        let slots = unsafe { &mut *self.slots.get() };
        let slot_map = unsafe { &mut *self.slot_by_ordinal.get() };
        let slot_idx = slot_map[lane_idx][ordinal];
        if slot_idx != ROUTE_SLOT_INDEX_NONE {
            let idx = slot_idx as usize;
            debug_assert!(
                slots[lane_idx][idx].coord == Some(coord),
                "route table slot mismatch"
            );
            return Some(idx);
        }
        let next_free = unsafe { &mut *self.next_free.get() };
        let idx = next_free[lane_idx] as usize;
        if idx >= ROUTE_SLOTS {
            return None;
        }
        next_free[lane_idx] = (idx as u16).saturating_add(1);
        slots[lane_idx][idx] = RouteSlot::assign(coord);
        slot_map[lane_idx][ordinal] = idx as u16;
        Some(idx)
    }

    #[inline]
    fn pending_lane_mask_bit(lane_idx: usize) -> u16 {
        1u16 << lane_idx
    }

    #[inline]
    fn route_pending_hint_label_idx(label: u32) -> Option<usize> {
        let idx = label as usize;
        (idx < ROUTE_PENDING_HINT_LABEL_CAPACITY).then_some(idx)
    }

    #[inline]
    fn pending_mask_slot_hash(coord: ScopeCoord) -> usize {
        Self::scope_ordinal(coord)
            .map(|ordinal| ordinal % ROUTE_SLOTS)
            .unwrap_or(0)
    }

    fn pending_mask_slot(&self, coord: ScopeCoord) -> Option<usize> {
        let slots = unsafe { &*self.pending_mask_slots.get() };
        let start = Self::pending_mask_slot_hash(coord);
        let mut probe = 0usize;
        while probe < ROUTE_SLOTS {
            let slot_idx = (start + probe) % ROUTE_SLOTS;
            match slots[slot_idx].coord {
                Some(existing) if existing == coord => return Some(slot_idx),
                None => return None,
                _ => {}
            }
            probe += 1;
        }
        None
    }

    fn pending_mask_slot_or_alloc(&self, coord: ScopeCoord) -> Option<usize> {
        let slots = unsafe { &mut *self.pending_mask_slots.get() };
        let start = Self::pending_mask_slot_hash(coord);
        let mut probe = 0usize;
        while probe < ROUTE_SLOTS {
            let slot_idx = (start + probe) % ROUTE_SLOTS;
            match slots[slot_idx].coord {
                Some(existing) if existing == coord => return Some(slot_idx),
                None => {
                    slots[slot_idx] = RoutePendingMaskSlot::assign(coord);
                    return Some(slot_idx);
                }
                _ => {}
            }
            probe += 1;
        }
        None
    }

    #[inline]
    fn bump_change_epoch(&self) {
        let epoch = unsafe { &mut *self.change_epoch.get() };
        let next = epoch.wrapping_add(1);
        *epoch = if next == 0 { 1 } else { next };
    }

    #[inline]
    pub(crate) fn change_epoch(&self) -> u32 {
        unsafe { *self.change_epoch.get() }
    }

    pub(crate) fn record(&self, lane: Lane, role_from: u8, scope: ScopeId, arm: u8) -> u16 {
        let coord = ScopeCoord::from_scope(scope).expect("route record requires structured scope");
        let lane_idx = Self::lane_idx(lane);
        let slot_idx =
            Self::slot_or_alloc(self, lane_idx, coord).expect("route table lane exhausted");
        let slots = unsafe { &mut *self.slots.get() };
        let entry = &mut slots[lane_idx][slot_idx].entry;
        let mut epoch = entry.epoch.wrapping_add(1);
        if epoch == 0 {
            epoch = 1;
        }
        entry.epoch = epoch;
        entry.arm = arm;
        if (role_from as usize) < ROLE_SLOTS {
            entry.last_seen[role_from as usize] = epoch;
        }
        if let Some(slot_idx) = Self::pending_mask_slot_or_alloc(self, coord) {
            let pending_mask_slots = unsafe { &mut *self.pending_mask_slots.get() };
            let lane_bit = Self::pending_lane_mask_bit(lane_idx);
            let mut role_idx = 0usize;
            while role_idx < ROLE_SLOTS {
                if role_idx == role_from as usize {
                    pending_mask_slots[slot_idx].lane_masks[role_idx] &= !lane_bit;
                } else {
                    pending_mask_slots[slot_idx].lane_masks[role_idx] |= lane_bit;
                }
                role_idx += 1;
            }
        }
        self.bump_change_epoch();

        let waiters = unsafe { &mut *self.waiters.get() };
        let row = &mut waiters[lane_idx];
        for waiter in row.iter_mut() {
            if let Some(waker) = waiter.take() {
                waker.wake();
            }
        }
        epoch
    }

    pub(crate) fn poll(
        &self,
        lane: Lane,
        role: u8,
        scope: ScopeId,
        cx: &mut Context<'_>,
    ) -> Poll<u8> {
        if (role as usize) >= ROLE_SLOTS {
            return Poll::Ready(0);
        }
        let coord = ScopeCoord::from_scope(scope).expect("route poll requires structured scope");
        let lane_idx = Self::lane_idx(lane);
        let slot_idx = match Self::slot_or_alloc(self, lane_idx, coord) {
            Some(idx) => idx,
            None => return Poll::Pending,
        };
        let slots = unsafe { &mut *self.slots.get() };
        let entry = &mut slots[lane_idx][slot_idx].entry;
        if entry.epoch != 0 && entry.last_seen[role as usize] != entry.epoch {
            entry.last_seen[role as usize] = entry.epoch;
            if let Some(slot_idx) = Self::pending_mask_slot(self, coord) {
                let pending_mask_slots = unsafe { &mut *self.pending_mask_slots.get() };
                pending_mask_slots[slot_idx].lane_masks[role as usize] &=
                    !Self::pending_lane_mask_bit(lane_idx);
            }
            self.bump_change_epoch();
            return Poll::Ready(entry.arm);
        }

        let waiters = unsafe { &mut *self.waiters.get() };
        let slot = &mut waiters[lane_idx][role as usize];
        *slot = Some(cx.waker().clone());
        Poll::Pending
    }

    pub(crate) fn acknowledge(&self, lane: Lane, role: u8, scope: ScopeId) -> Option<u8> {
        if (role as usize) >= ROLE_SLOTS {
            return None;
        }
        let coord = ScopeCoord::from_scope(scope)?;
        let lane_idx = Self::lane_idx(lane);
        let slot_idx = Self::slot_for_scope(self, lane_idx, coord)?;
        let slots = unsafe { &mut *self.slots.get() };
        let entry = &mut slots[lane_idx][slot_idx].entry;
        if entry.epoch == 0 {
            return None;
        }
        let role_idx = role as usize;
        if entry.last_seen[role_idx] == entry.epoch {
            return None;
        }
        entry.last_seen[role_idx] = entry.epoch;
        if let Some(slot_idx) = Self::pending_mask_slot(self, coord) {
            let pending_mask_slots = unsafe { &mut *self.pending_mask_slots.get() };
            pending_mask_slots[slot_idx].lane_masks[role_idx] &=
                !Self::pending_lane_mask_bit(lane_idx);
        }
        self.bump_change_epoch();
        Some(entry.arm)
    }

    pub(crate) fn peek(&self, lane: Lane, role: u8, scope: ScopeId) -> Option<u8> {
        if (role as usize) >= ROLE_SLOTS {
            return None;
        }
        let coord = ScopeCoord::from_scope(scope)?;
        let lane_idx = Self::lane_idx(lane);
        let slot_idx = Self::slot_for_scope(self, lane_idx, coord)?;
        let slots = unsafe { &*self.slots.get() };
        let entry = &slots[lane_idx][slot_idx].entry;
        let role_idx = role as usize;
        (entry.epoch != 0 && entry.last_seen[role_idx] != entry.epoch).then_some(entry.arm)
    }

    pub(crate) fn pending_lane_mask(&self, role: u8, scope: ScopeId) -> u16 {
        if (role as usize) >= ROLE_SLOTS {
            return 0;
        }
        let coord = match ScopeCoord::from_scope(scope) {
            Some(coord) => coord,
            None => return 0,
        };
        let Some(slot_idx) = Self::pending_mask_slot(self, coord) else {
            return 0;
        };
        let pending_mask_slots = unsafe { &*self.pending_mask_slots.get() };
        pending_mask_slots[slot_idx].lane_masks[role as usize]
    }

    pub(crate) fn update_pending_hint_lane_masks(&self, lane: Lane, before: u128, after: u128) {
        if before == after {
            return;
        }
        let lane_bit = Self::pending_lane_mask_bit(Self::lane_idx(lane));
        let pending_hint_lane_masks = unsafe { &mut *self.pending_hint_lane_masks.get() };

        let mut removed = before & !after;
        while removed != 0 {
            let label = removed.trailing_zeros();
            removed &= !(1u128 << label);
            if let Some(idx) = Self::route_pending_hint_label_idx(label) {
                pending_hint_lane_masks[idx] &= !lane_bit;
            }
        }

        let mut added = after & !before;
        while added != 0 {
            let label = added.trailing_zeros();
            added &= !(1u128 << label);
            if let Some(idx) = Self::route_pending_hint_label_idx(label) {
                pending_hint_lane_masks[idx] |= lane_bit;
            }
        }
        self.bump_change_epoch();
    }

    pub(crate) fn pending_hint_lane_mask_for_labels(&self, label_mask: u128) -> u16 {
        let pending_hint_lane_masks = unsafe { &*self.pending_hint_lane_masks.get() };
        let mut mask = label_mask;
        let mut lanes = 0u16;
        while mask != 0 {
            let label = mask.trailing_zeros();
            mask &= !(1u128 << label);
            if let Some(idx) = Self::route_pending_hint_label_idx(label) {
                lanes |= pending_hint_lane_masks[idx];
            }
        }
        lanes
    }

    pub(crate) fn reset_lane(&self, lane: Lane) {
        let slots = unsafe { &mut *self.slots.get() };
        let lane_idx = Self::lane_idx(lane);
        let pending_mask_slots = unsafe { &mut *self.pending_mask_slots.get() };
        let lane_bit = Self::pending_lane_mask_bit(lane_idx);
        let mut slot_idx = 0usize;
        while slot_idx < ROUTE_SLOTS {
            if let Some(coord) = slots[lane_idx][slot_idx].coord
                && let Some(summary_slot_idx) = Self::pending_mask_slot(self, coord)
            {
                let mut role_idx = 0usize;
                while role_idx < ROLE_SLOTS {
                    pending_mask_slots[summary_slot_idx].lane_masks[role_idx] &= !lane_bit;
                    role_idx += 1;
                }
            }
            slot_idx += 1;
        }
        slots[lane_idx] = [RouteSlot::empty(); ROUTE_SLOTS];

        let slot_map = unsafe { &mut *self.slot_by_ordinal.get() };
        slot_map[lane_idx] = [ROUTE_SLOT_INDEX_NONE; ROUTE_SCOPE_ORDINAL_CAPACITY];
        let next_free = unsafe { &mut *self.next_free.get() };
        next_free[lane_idx] = 0;
        let pending_hint_lane_masks = unsafe { &mut *self.pending_hint_lane_masks.get() };
        let mut label_idx = 0usize;
        while label_idx < ROUTE_PENDING_HINT_LABEL_CAPACITY {
            pending_hint_lane_masks[label_idx] &= !lane_bit;
            label_idx += 1;
        }

        let waiters = unsafe { &mut *self.waiters.get() };
        let row = &mut waiters[lane_idx];
        for waiter in row.iter_mut() {
            *waiter = None;
        }
        self.bump_change_epoch();
    }
}

#[cfg(test)]
mod tests {
    use super::RouteTable;
    use crate::{control::types::Lane, global::const_dsl::ScopeId};

    #[test]
    fn route_table_peek_is_non_consuming() {
        let table = RouteTable::new();
        let lane = Lane::new(0);
        let scope = ScopeId::route(9);

        assert_eq!(table.peek(lane, 1, scope), None);
        table.record(lane, 0, scope, 1);
        assert_eq!(table.peek(lane, 1, scope), Some(1));
        assert_eq!(table.peek(lane, 1, scope), Some(1));
        assert_eq!(table.acknowledge(lane, 1, scope), Some(1));
        assert_eq!(table.peek(lane, 1, scope), None);
    }

    #[test]
    fn route_table_pending_lane_mask_tracks_unacked_decisions() {
        let table = RouteTable::new();
        let lane0 = Lane::new(0);
        let lane2 = Lane::new(2);
        let scope = ScopeId::route(9);

        assert_eq!(table.pending_lane_mask(1, scope), 0);

        table.record(lane0, 0, scope, 1);
        table.record(lane2, 0, scope, 1);
        assert_eq!(table.pending_lane_mask(0, scope), 0);
        assert_eq!(table.pending_lane_mask(1, scope), (1u16 << 0) | (1u16 << 2));

        assert_eq!(table.acknowledge(lane0, 1, scope), Some(1));
        assert_eq!(table.pending_lane_mask(1, scope), 1u16 << 2);

        table.record(lane0, 0, scope, 0);
        assert_eq!(table.pending_lane_mask(1, scope), (1u16 << 0) | (1u16 << 2));

        table.reset_lane(lane2);
        assert_eq!(table.pending_lane_mask(1, scope), 1u16 << 0);
    }

    #[test]
    fn route_table_change_epoch_tracks_route_and_hint_updates() {
        let table = RouteTable::new();
        let lane = Lane::new(0);
        let scope = ScopeId::route(9);

        let initial = table.change_epoch();
        table.record(lane, 0, scope, 1);
        let after_record = table.change_epoch();
        assert_ne!(after_record, initial);

        assert_eq!(table.acknowledge(lane, 1, scope), Some(1));
        let after_ack = table.change_epoch();
        assert_ne!(after_ack, after_record);

        table.update_pending_hint_lane_masks(lane, 0, 1u128 << 25);
        let after_hint = table.change_epoch();
        assert_ne!(after_hint, after_ack);

        table.reset_lane(lane);
        assert_ne!(table.change_epoch(), after_hint);
    }

    #[test]
    fn route_table_hint_lane_mask_tracks_buffered_labels() {
        let table = RouteTable::new();
        let lane0 = Lane::new(0);
        let lane2 = Lane::new(2);

        assert_eq!(table.pending_hint_lane_mask_for_labels(1u128 << 25), 0);

        table.update_pending_hint_lane_masks(lane0, 0, (1u128 << 25) | (1u128 << 41));
        assert_eq!(
            table.pending_hint_lane_mask_for_labels(1u128 << 25),
            1u16 << 0
        );
        assert_eq!(
            table.pending_hint_lane_mask_for_labels((1u128 << 25) | (1u128 << 41)),
            1u16 << 0
        );

        table.update_pending_hint_lane_masks(lane2, 0, 1u128 << 41);
        assert_eq!(
            table.pending_hint_lane_mask_for_labels(1u128 << 41),
            (1u16 << 0) | (1u16 << 2)
        );

        table.update_pending_hint_lane_masks(lane0, (1u128 << 25) | (1u128 << 41), 1u128 << 41);
        assert_eq!(table.pending_hint_lane_mask_for_labels(1u128 << 25), 0);
        assert_eq!(
            table.pending_hint_lane_mask_for_labels(1u128 << 41),
            (1u16 << 0) | (1u16 << 2)
        );

        table.reset_lane(lane2);
        assert_eq!(
            table.pending_hint_lane_mask_for_labels(1u128 << 41),
            1u16 << 0
        );
    }
}

/// Fence counter table (per-lane).
///
/// Tracks tx/rx fence numbers for each lane (for splice operations).
pub(crate) struct FenceTable {
    tx: UnsafeCell<[Option<u32>; LANES_MAX as usize]>,
    rx: UnsafeCell<[Option<u32>; LANES_MAX as usize]>,
    _no_send_sync: PhantomData<*mut ()>,
}

impl Default for FenceTable {
    fn default() -> Self {
        Self::new()
    }
}

impl FenceTable {
    pub(crate) const fn new() -> Self {
        Self {
            tx: UnsafeCell::new([None; LANES_MAX as usize]),
            rx: UnsafeCell::new([None; LANES_MAX as usize]),
            _no_send_sync: PhantomData,
        }
    }

    /// Record tx fence for a lane.
    #[inline]
    pub(crate) fn record_tx(&self, lane: Lane, value: u32) {
        unsafe {
            (*self.tx.get())[lane.raw() as usize] = Some(value);
        }
    }

    /// Record rx fence for a lane.
    #[inline]
    pub(crate) fn record_rx(&self, lane: Lane, value: u32) {
        unsafe {
            (*self.rx.get())[lane.raw() as usize] = Some(value);
        }
    }

    /// Reset lane.
    #[inline]
    pub(crate) fn reset_lane(&self, lane: Lane) {
        unsafe {
            (*self.tx.get())[lane.raw() as usize] = None;
            (*self.rx.get())[lane.raw() as usize] = None;
        }
    }
}

/// Ack counter table (per-lane).
///
/// Tracks cancel_begin, cancel_ack, and last_gen for AMPST cancellation protocol.
pub(crate) struct AckTable {
    last_ack_gen: UnsafeCell<[Option<u16>; LANES_MAX as usize]>,
    cancel_begin: UnsafeCell<[u32; LANES_MAX as usize]>,
    cancel_ack: UnsafeCell<[u32; LANES_MAX as usize]>,
    _no_send_sync: PhantomData<*mut ()>,
}

impl Default for AckTable {
    fn default() -> Self {
        Self::new()
    }
}

impl AckTable {
    pub(crate) const fn new() -> Self {
        Self {
            last_ack_gen: UnsafeCell::new([None; LANES_MAX as usize]),
            cancel_begin: UnsafeCell::new([0; LANES_MAX as usize]),
            cancel_ack: UnsafeCell::new([0; LANES_MAX as usize]),
            _no_send_sync: PhantomData,
        }
    }

    /// Record cancel_begin event.
    #[inline]
    pub(crate) fn record_cancel_begin(&self, lane: Lane) {
        unsafe {
            let counters = &mut *self.cancel_begin.get();
            counters[lane.raw() as usize] = counters[lane.raw() as usize].saturating_add(1);
        }
    }

    /// Record cancel_ack event.
    #[inline]
    pub(crate) fn record_cancel_ack(&self, lane: Lane, generation: Generation) {
        unsafe {
            let slots = &mut *self.last_ack_gen.get();
            let counters = &mut *self.cancel_ack.get();
            let idx = lane.raw() as usize;
            let slot = &mut slots[idx];
            match *slot {
                None => {
                    *slot = Some(generation.raw());
                }
                Some(prev) if generation.raw() > prev => {
                    *slot = Some(generation.raw());
                }
                _ => {}
            }
            counters[idx] = counters[idx].saturating_add(1);
        }
    }

    /// Reset lane.
    #[inline]
    pub(crate) fn reset_lane(&self, lane: Lane) {
        unsafe {
            (*self.last_ack_gen.get())[lane.raw() as usize] = None;
            (*self.cancel_begin.get())[lane.raw() as usize] = 0;
            (*self.cancel_ack.get())[lane.raw() as usize] = 0;
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PolicyKey {
    eff_index: EffIndex,
    tag: u8,
}

struct PolicySlot {
    policies: ArrayMap<PolicyKey, PolicyMode, CONTROL_PLAN_SLOTS>,
}

impl PolicySlot {
    const fn new() -> Self {
        Self {
            policies: ArrayMap::new(),
        }
    }

    fn register(
        &mut self,
        eff_index: EffIndex,
        tag: u8,
        policy: PolicyMode,
    ) -> Result<(), PolicyMode> {
        let key = PolicyKey { eff_index, tag };
        self.policies.insert(key, policy)
    }

    fn get(&self, eff_index: EffIndex, tag: u8) -> Option<PolicyMode> {
        let key = PolicyKey { eff_index, tag };
        self.policies.get(&key).copied()
    }

    fn reset(&mut self) {
        self.policies.clear();
    }
}

pub(crate) struct PolicyTable {
    lanes: UnsafeCell<[PolicySlot; LANES_MAX as usize]>,
    _no_send_sync: PhantomData<*mut ()>,
}

impl Default for PolicyTable {
    fn default() -> Self {
        Self::new()
    }
}

impl PolicyTable {
    pub(crate) const fn new() -> Self {
        Self {
            lanes: UnsafeCell::new([const { PolicySlot::new() }; LANES_MAX as usize]),
            _no_send_sync: PhantomData,
        }
    }

    pub(crate) fn register(
        &self,
        lane: Lane,
        eff_index: EffIndex,
        tag: u8,
        policy: PolicyMode,
    ) -> Result<(), PolicyMode> {
        if policy.is_static() {
            return Ok(());
        }
        unsafe {
            let slots = &mut *self.lanes.get();
            slots[lane.raw() as usize].register(eff_index, tag, policy)
        }
    }

    pub(crate) fn get(&self, lane: Lane, eff_index: EffIndex, tag: u8) -> Option<PolicyMode> {
        unsafe {
            let slots = &*self.lanes.get();
            slots[lane.raw() as usize].get(eff_index, tag)
        }
    }

    pub(crate) fn reset_lane(&self, lane: Lane) {
        unsafe {
            let slots = &mut *self.lanes.get();
            slots[lane.raw() as usize].reset();
        }
    }
}

/// Per-lane capability table for the effect VM.
pub(crate) struct VmCapsTable {
    lanes: UnsafeCell<[CapsMask; LANES_MAX as usize]>,
    _no_send_sync: PhantomData<*mut ()>,
}

impl Default for VmCapsTable {
    fn default() -> Self {
        Self::new()
    }
}

impl VmCapsTable {
    pub(crate) const fn new() -> Self {
        Self {
            lanes: UnsafeCell::new([CapsMask::allow_all(); LANES_MAX as usize]),
            _no_send_sync: PhantomData,
        }
    }

    /// Set capability bits for a lane.
    #[inline]
    pub(crate) fn set(&self, lane: Lane, caps: CapsMask) {
        unsafe {
            (*self.lanes.get())[lane.raw() as usize] = caps;
        }
    }

    /// Get capability bits for a lane.
    #[inline]
    pub(crate) fn get(&self, lane: Lane) -> CapsMask {
        unsafe { (*self.lanes.get())[lane.raw() as usize] }
    }

    /// Reset lane (clear permissions).
    #[inline]
    pub(crate) fn reset_lane(&self, lane: Lane) {
        unsafe {
            (*self.lanes.get())[lane.raw() as usize] = CapsMask::allow_all();
        }
    }
}

/// Checkpoint table (per-lane).
///
/// Tracks last checkpoint epoch and consumption status for rollback operations.
pub(crate) struct CheckpointTable {
    last_checkpoint: UnsafeCell<[Option<u16>; LANES_MAX as usize]>,
    consumed: UnsafeCell<[bool; LANES_MAX as usize]>,
    _no_send_sync: PhantomData<*mut ()>,
}

impl Default for CheckpointTable {
    fn default() -> Self {
        Self::new()
    }
}

impl CheckpointTable {
    pub(crate) const fn new() -> Self {
        Self {
            last_checkpoint: UnsafeCell::new([None; LANES_MAX as usize]),
            consumed: UnsafeCell::new([false; LANES_MAX as usize]),
            _no_send_sync: PhantomData,
        }
    }

    /// Record a checkpoint.
    #[inline]
    pub(crate) fn record(&self, lane: Lane, checkpoint: Generation) {
        unsafe {
            (*self.last_checkpoint.get())[lane.raw() as usize] = Some(checkpoint.raw());
            (*self.consumed.get())[lane.raw() as usize] = false;
        }
    }

    /// Get last checkpoint for a lane.
    #[inline]
    pub(crate) fn last(&self, lane: Lane) -> Option<Generation> {
        unsafe { (*self.last_checkpoint.get())[lane.raw() as usize].map(Generation::new) }
    }

    /// Mark checkpoint as consumed.
    #[inline]
    pub(crate) fn mark_consumed(&self, lane: Lane) {
        unsafe {
            (*self.consumed.get())[lane.raw() as usize] = true;
        }
    }

    /// Check if checkpoint is consumed.
    #[inline]
    pub(crate) fn is_consumed(&self, lane: Lane) -> bool {
        unsafe { (*self.consumed.get())[lane.raw() as usize] }
    }

    /// Reset lane.
    #[inline]
    pub(crate) fn reset_lane(&self, lane: Lane) {
        unsafe {
            (*self.last_checkpoint.get())[lane.raw() as usize] = None;
            (*self.consumed.get())[lane.raw() as usize] = false;
        }
    }
}
