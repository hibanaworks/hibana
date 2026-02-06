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

use super::{
    error::{GenError, GenerationRecord},
    types::{Generation, Lane},
};
use crate::{
    control::{cap::CapsMask, lease::map::ArrayMap},
    eff::{self, EffIndex},
    global::const_dsl::{HandlePlan, ScopeId, ScopeKind},
    observe::FenceCounters,
    runtime::consts::LANES_MAX,
};

const ROLE_SLOTS: usize = LANES_MAX as usize;
const LOOP_SLOTS: usize = eff::meta::MAX_EFF_NODES;
const ROUTE_SLOTS: usize = eff::meta::MAX_EFF_NODES;
const ROUTE_SCOPE_ORDINAL_CAPACITY: usize = ScopeId::ORDINAL_CAPACITY as usize;
const ROUTE_SLOT_INDEX_NONE: u16 = u16::MAX;
const CONTROL_PLAN_SLOTS: usize = 128;

/// Generation counter table (per-lane).
///
/// Tracks the last seen generation number for each lane to ensure monotonic updates.
pub struct GenTable {
    lanes: UnsafeCell<[Option<u16>; LANES_MAX as usize]>,
    _no_send_sync: PhantomData<*mut ()>,
}

impl Default for GenTable {
    fn default() -> Self {
        Self::new()
    }
}

impl GenTable {
    pub const fn new() -> Self {
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
    pub fn check_and_update(&self, lane: Lane, new: Generation) -> Result<(), GenError> {
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
    pub fn last(&self, lane: Lane) -> Option<Generation> {
        unsafe {
            let buf = &*self.lanes.get();
            buf[lane.raw() as usize].map(Generation::new)
        }
    }

    /// Reset lane (for release).
    #[inline]
    pub fn reset_lane(&self, lane: Lane) {
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

pub struct LoopTable {
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
    pub fn new() -> Self {
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

    pub fn acknowledge(&self, lane: Lane, role: u8, idx: u8) {
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

    pub fn reset_lane(&self, lane: Lane) {
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

pub struct RouteTable {
    slots: UnsafeCell<[[RouteSlot; ROUTE_SLOTS]; LANES_MAX as usize]>,
    slot_by_ordinal: UnsafeCell<[[u16; ROUTE_SCOPE_ORDINAL_CAPACITY]; LANES_MAX as usize]>,
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
    pub fn new() -> Self {
        Self {
            slots: UnsafeCell::new([[RouteSlot::empty(); ROUTE_SLOTS]; LANES_MAX as usize]),
            slot_by_ordinal: UnsafeCell::new([
                [ROUTE_SLOT_INDEX_NONE; ROUTE_SCOPE_ORDINAL_CAPACITY];
                LANES_MAX as usize
            ]),
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
        Some(entry.arm)
    }

    pub fn reset_lane(&self, lane: Lane) {
        let slots = unsafe { &mut *self.slots.get() };
        let lane_idx = Self::lane_idx(lane);
        slots[lane_idx] = [RouteSlot::empty(); ROUTE_SLOTS];

        let slot_map = unsafe { &mut *self.slot_by_ordinal.get() };
        slot_map[lane_idx] = [ROUTE_SLOT_INDEX_NONE; ROUTE_SCOPE_ORDINAL_CAPACITY];
        let next_free = unsafe { &mut *self.next_free.get() };
        next_free[lane_idx] = 0;

        let waiters = unsafe { &mut *self.waiters.get() };
        let row = &mut waiters[lane_idx];
        for waiter in row.iter_mut() {
            *waiter = None;
        }
    }
}

/// Fence counter table (per-lane).
///
/// Tracks tx/rx fence numbers for each lane (for splice operations).
pub struct FenceTable {
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
    pub const fn new() -> Self {
        Self {
            tx: UnsafeCell::new([None; LANES_MAX as usize]),
            rx: UnsafeCell::new([None; LANES_MAX as usize]),
            _no_send_sync: PhantomData,
        }
    }

    /// Record tx fence for a lane.
    #[inline]
    pub fn record_tx(&self, lane: Lane, value: u32) {
        unsafe {
            (*self.tx.get())[lane.raw() as usize] = Some(value);
        }
    }

    /// Record rx fence for a lane.
    #[inline]
    pub fn record_rx(&self, lane: Lane, value: u32) {
        unsafe {
            (*self.rx.get())[lane.raw() as usize] = Some(value);
        }
    }

    /// Get last tx fence for a lane.
    #[inline]
    pub fn last_tx(&self, lane: Lane) -> Option<u32> {
        unsafe { (*self.tx.get())[lane.raw() as usize] }
    }

    /// Get last rx fence for a lane.
    #[inline]
    pub fn last_rx(&self, lane: Lane) -> Option<u32> {
        unsafe { (*self.rx.get())[lane.raw() as usize] }
    }

    /// Get fence counters for a lane.
    #[inline]
    pub fn get(&self, lane: Lane) -> FenceCounters {
        FenceCounters {
            tx: self.last_tx(lane),
            rx: self.last_rx(lane),
        }
    }

    /// Reset lane.
    #[inline]
    pub fn reset_lane(&self, lane: Lane) {
        unsafe {
            (*self.tx.get())[lane.raw() as usize] = None;
            (*self.rx.get())[lane.raw() as usize] = None;
        }
    }
}

/// Ack counter table (per-lane).
///
/// Tracks cancel_begin, cancel_ack, and last_gen for AMPST cancellation protocol.
pub struct AckTable {
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
    pub const fn new() -> Self {
        Self {
            last_ack_gen: UnsafeCell::new([None; LANES_MAX as usize]),
            cancel_begin: UnsafeCell::new([0; LANES_MAX as usize]),
            cancel_ack: UnsafeCell::new([0; LANES_MAX as usize]),
            _no_send_sync: PhantomData,
        }
    }

    /// Record cancel_begin event.
    #[inline]
    pub fn record_cancel_begin(&self, lane: Lane) {
        unsafe {
            let counters = &mut *self.cancel_begin.get();
            counters[lane.raw() as usize] = counters[lane.raw() as usize].saturating_add(1);
        }
    }

    /// Record cancel_ack event.
    #[inline]
    pub fn record_cancel_ack(&self, lane: Lane, generation: Generation) {
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

    /// Get last ack generation for a lane.
    #[inline]
    pub fn last_gen(&self, lane: Lane) -> Option<Generation> {
        unsafe { (*self.last_ack_gen.get())[lane.raw() as usize].map(Generation::new) }
    }

    /// Alias for last_gen (for compatibility).
    #[inline]
    pub fn last_ack_gen(&self, lane: Lane) -> Option<Generation> {
        self.last_gen(lane)
    }

    /// Get cancel_begin counter.
    #[inline]
    pub fn cancel_begin(&self, lane: Lane) -> u32 {
        unsafe { (*self.cancel_begin.get())[lane.raw() as usize] }
    }

    /// Get cancel_ack counter.
    #[inline]
    pub fn cancel_ack(&self, lane: Lane) -> u32 {
        unsafe { (*self.cancel_ack.get())[lane.raw() as usize] }
    }

    /// Reset lane.
    #[inline]
    pub fn reset_lane(&self, lane: Lane) {
        unsafe {
            (*self.last_ack_gen.get())[lane.raw() as usize] = None;
            (*self.cancel_begin.get())[lane.raw() as usize] = 0;
            (*self.cancel_ack.get())[lane.raw() as usize] = 0;
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PlanKey {
    eff_index: EffIndex,
    tag: u8,
}

struct ControlPlanSlot {
    plans: ArrayMap<PlanKey, HandlePlan, CONTROL_PLAN_SLOTS>,
}

impl ControlPlanSlot {
    const fn new() -> Self {
        Self {
            plans: ArrayMap::new(),
        }
    }

    fn register(
        &mut self,
        eff_index: EffIndex,
        tag: u8,
        plan: HandlePlan,
    ) -> Result<(), HandlePlan> {
        let key = PlanKey { eff_index, tag };
        self.plans.insert(key, plan)
    }

    fn get(&self, eff_index: EffIndex, tag: u8) -> Option<HandlePlan> {
        let key = PlanKey { eff_index, tag };
        self.plans.get(&key).copied()
    }

    fn reset(&mut self) {
        self.plans.clear();
    }
}

pub struct ControlPlanTable {
    lanes: UnsafeCell<[ControlPlanSlot; LANES_MAX as usize]>,
    _no_send_sync: PhantomData<*mut ()>,
}

impl Default for ControlPlanTable {
    fn default() -> Self {
        Self::new()
    }
}

impl ControlPlanTable {
    pub const fn new() -> Self {
        Self {
            lanes: UnsafeCell::new([const { ControlPlanSlot::new() }; LANES_MAX as usize]),
            _no_send_sync: PhantomData,
        }
    }

    pub fn register(
        &self,
        lane: Lane,
        eff_index: EffIndex,
        tag: u8,
        plan: HandlePlan,
    ) -> Result<(), HandlePlan> {
        if plan.is_none() {
            return Ok(());
        }
        unsafe {
            let slots = &mut *self.lanes.get();
            slots[lane.raw() as usize].register(eff_index, tag, plan)
        }
    }

    pub fn get(&self, lane: Lane, eff_index: EffIndex, tag: u8) -> Option<HandlePlan> {
        unsafe {
            let slots = &*self.lanes.get();
            slots[lane.raw() as usize].get(eff_index, tag)
        }
    }

    pub fn reset_lane(&self, lane: Lane) {
        unsafe {
            let slots = &mut *self.lanes.get();
            slots[lane.raw() as usize].reset();
        }
    }
}

/// Per-lane capability table for the effect VM.
pub struct VmCapsTable {
    lanes: UnsafeCell<[CapsMask; LANES_MAX as usize]>,
    _no_send_sync: PhantomData<*mut ()>,
}

impl Default for VmCapsTable {
    fn default() -> Self {
        Self::new()
    }
}

impl VmCapsTable {
    pub const fn new() -> Self {
        Self {
            lanes: UnsafeCell::new([CapsMask::allow_all(); LANES_MAX as usize]),
            _no_send_sync: PhantomData,
        }
    }

    /// Set capability bits for a lane.
    #[inline]
    pub fn set(&self, lane: Lane, caps: CapsMask) {
        unsafe {
            (*self.lanes.get())[lane.raw() as usize] = caps;
        }
    }

    /// Get capability bits for a lane.
    #[inline]
    pub fn get(&self, lane: Lane) -> CapsMask {
        unsafe { (*self.lanes.get())[lane.raw() as usize] }
    }

    /// Reset lane (clear permissions).
    #[inline]
    pub fn reset_lane(&self, lane: Lane) {
        unsafe {
            (*self.lanes.get())[lane.raw() as usize] = CapsMask::allow_all();
        }
    }
}

/// Checkpoint table (per-lane).
///
/// Tracks last checkpoint epoch and consumption status for rollback operations.
pub struct CheckpointTable {
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
    pub const fn new() -> Self {
        Self {
            last_checkpoint: UnsafeCell::new([None; LANES_MAX as usize]),
            consumed: UnsafeCell::new([false; LANES_MAX as usize]),
            _no_send_sync: PhantomData,
        }
    }

    /// Record a checkpoint.
    #[inline]
    pub fn record(&self, lane: Lane, checkpoint: Generation) {
        unsafe {
            (*self.last_checkpoint.get())[lane.raw() as usize] = Some(checkpoint.raw());
            (*self.consumed.get())[lane.raw() as usize] = false;
        }
    }

    /// Get last checkpoint for a lane.
    #[inline]
    pub fn last(&self, lane: Lane) -> Option<Generation> {
        unsafe { (*self.last_checkpoint.get())[lane.raw() as usize].map(Generation::new) }
    }

    /// Mark checkpoint as consumed.
    #[inline]
    pub fn mark_consumed(&self, lane: Lane) {
        unsafe {
            (*self.consumed.get())[lane.raw() as usize] = true;
        }
    }

    /// Check if checkpoint is consumed.
    #[inline]
    pub fn is_consumed(&self, lane: Lane) -> bool {
        unsafe { (*self.consumed.get())[lane.raw() as usize] }
    }

    /// Reset lane.
    #[inline]
    pub fn reset_lane(&self, lane: Lane) {
        unsafe {
            (*self.last_checkpoint.get())[lane.raw() as usize] = None;
            (*self.consumed.get())[lane.raw() as usize] = false;
        }
    }
}
