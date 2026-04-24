//! Rendezvous (control plane) primitives.
//!
//! The rendezvous component owns the association tables that map session
//! identifiers to transport lanes. A fully-fledged implementation would manage
//! topology/delegate bookkeeping and generation counters; the current version
//! keeps just enough structure to support endpoint scaffolding while leaving
//! clear extension points.

use core::{cell::Cell, marker::PhantomData, ops::Range};

use super::{
    association::AssocTable,
    capability::{CapEntry, CapReleaseCtx, CapTable},
    error::{
        CapError, GenError, GenerationRecord, RendezvousError, StateRestoreError, TopologyError,
        TxAbortError, TxCommitError,
    },
    port::Port,
    slots::SlotArena,
    tables::{
        GenTable, LoopTable, PolicyTable, RouteTable, SnapshotFinalization, StateSnapshotTable,
    },
    topology::{PendingTopology, TopologyStateTable},
};
use crate::{
    control::{
        automaton::txn::{NoopTap, Txn},
        brand::{self, Guard},
        cap::mint::{
            CapShot, ControlOp, EndpointResource, GenericCapToken, NonceSeed, ResourceKind,
            VerifiedCap,
        },
        cluster::{
            core::{CpCommand, EffectRunner, TopologyOperands},
            error::CpError,
        },
        types::{IncreasingGen, One},
    },
    eff::EffIndex,
    endpoint::affine::LaneGuard,
    global::compiled::{
        images::{CompiledProgramFacts, CompiledRoleImage},
        lowering::{LoweringSummary, ProgramStamp},
        materialize::RoleLoweringScratch,
    },
    global::const_dsl::{ControlScopeKind, PolicyMode},
    observe::core::{TapEvent, TapRing, emit},
    observe::{
        events::{DelegBegin, LaneRelease, RawEvent, StateRestoreOk},
        ids,
    },
    policy_runtime::{self, PolicySlot},
    runtime::config::{Clock, Config, ConfigParts, CounterClock},
    runtime::consts::{DefaultLabelUniverse, LabelUniverse},
    transport::{Transport, TransportEventKind, TransportMetrics},
};

use super::topology::{LocalTopologyInvariant, TopologySessionState};
use crate::control::automaton::distributed::{TopologyAck, TopologyIntent};
use crate::control::cluster::effects::control_op_tap_event_id;
use crate::control::types::{Generation, Lane, RendezvousId, SessionId};

#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct EndpointLeaseId(u16);

impl EndpointLeaseId {
    pub(crate) const ZERO: Self = Self(0);
}

impl From<u8> for EndpointLeaseId {
    #[inline]
    fn from(value: u8) -> Self {
        Self(value.into())
    }
}

impl From<u16> for EndpointLeaseId {
    #[inline]
    fn from(value: u16) -> Self {
        Self(value)
    }
}

impl From<EndpointLeaseId> for u16 {
    #[inline]
    fn from(value: EndpointLeaseId) -> Self {
        value.0
    }
}

impl From<EndpointLeaseId> for u32 {
    #[inline]
    fn from(value: EndpointLeaseId) -> Self {
        value.0.into()
    }
}

impl From<EndpointLeaseId> for usize {
    #[inline]
    fn from(value: EndpointLeaseId) -> Self {
        value.0.into()
    }
}

impl TryFrom<usize> for EndpointLeaseId {
    type Error = core::num::TryFromIntError;

    #[inline]
    fn try_from(value: usize) -> Result<Self, Self::Error> {
        u16::try_from(value).map(Self)
    }
}

impl core::fmt::Display for EndpointLeaseId {
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CompiledProgramSlot {
    stamp: ProgramStamp,
    offset: u32,
    len: u32,
    pins: u16,
    occupied: bool,
}

impl CompiledProgramSlot {
    const EMPTY: Self = Self {
        stamp: ProgramStamp::EMPTY,
        offset: 0,
        len: 0,
        pins: 0,
        occupied: false,
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RoleImageSlot {
    stamp: ProgramStamp,
    role: u8,
    offset: u32,
    len: u32,
    pins: u16,
    occupied: bool,
}

impl RoleImageSlot {
    const EMPTY: Self = Self {
        stamp: ProgramStamp::EMPTY,
        role: u8::MAX,
        offset: 0,
        len: 0,
        pins: 0,
        occupied: false,
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EndpointResidentBudget {
    pub(crate) route_frame_slots: u16,
    pub(crate) loop_slots: u16,
    pub(crate) cap_entries: u16,
    pub(crate) route_lane_slots: u8,
}

impl EndpointResidentBudget {
    pub(crate) const ZERO: Self = Self {
        route_frame_slots: 0,
        loop_slots: 0,
        cap_entries: 0,
        route_lane_slots: 0,
    };

    #[inline]
    pub(crate) const fn with_route_storage(
        route_frame_slots: usize,
        route_lane_slots: usize,
        loop_slots: usize,
        cap_entries: usize,
    ) -> Self {
        Self {
            route_frame_slots: if route_frame_slots > u16::MAX as usize {
                u16::MAX
            } else {
                route_frame_slots as u16
            },
            loop_slots: if loop_slots > u16::MAX as usize {
                u16::MAX
            } else {
                loop_slots as u16
            },
            cap_entries: if cap_entries > u16::MAX as usize {
                u16::MAX
            } else {
                cap_entries as u16
            },
            route_lane_slots: if route_lane_slots > u8::MAX as usize {
                u8::MAX
            } else {
                route_lane_slots as u8
            },
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct EndpointLeaseSlot {
    pub(crate) generation: u32,
    pub(crate) offset: u32,
    pub(crate) len: u32,
    pub(crate) resident_budget: EndpointResidentBudget,
    pub(crate) program_image_slot: u8,
    pub(crate) role_image_slot: u8,
    pub(crate) public_endpoint: bool,
    pub(crate) occupied: bool,
}

impl EndpointLeaseSlot {
    const EMPTY: Self = Self {
        generation: 0,
        offset: 0,
        len: 0,
        resident_budget: EndpointResidentBudget::ZERO,
        program_image_slot: u8::MAX,
        role_image_slot: u8::MAX,
        public_endpoint: false,
        occupied: false,
    };
}

const FREE_REGION_CAPACITY: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FreeRegion {
    offset: u32,
    len: u32,
    occupied: bool,
}

impl FreeRegion {
    const EMPTY: Self = Self {
        offset: 0,
        len: 0,
        occupied: false,
    };
}

pub(crate) struct Rendezvous<
    'rv,
    'cfg,
    T: Transport,
    U: LabelUniverse = DefaultLabelUniverse,
    C: Clock = CounterClock,
    E: crate::control::cap::mint::EpochTable = crate::control::cap::mint::EpochTbl,
> where
    'cfg: 'rv,
{
    brand_marker: PhantomData<brand::Brand<'rv>>,
    id: RendezvousId,
    tap: TapRing<'cfg>,
    slab: *mut [u8],
    slab_marker: PhantomData<&'cfg mut [u8]>,
    image_frontier: u32,
    scratch_reserved_bytes: u32,
    program_images: *mut CompiledProgramSlot,
    role_images: *mut RoleImageSlot,
    endpoint_leases: *mut EndpointLeaseSlot,
    image_slot_capacity: u8,
    endpoint_lease_capacity: EndpointLeaseId,
    runtime_frontier: u32,
    free_regions: [FreeRegion; FREE_REGION_CAPACITY],
    lane_range: Range<u32>,
    universe_marker: PhantomData<U>,
    transport: T,
    r#gen: GenTable,
    assoc: AssocTable,
    state_snapshots: StateSnapshotTable,
    topology: TopologyStateTable,
    cap_nonce: Cell<u64>,
    cap_revision: Cell<u64>,
    caps: CapTable,
    loops: LoopTable,
    routes: RouteTable,
    policies: PolicyTable,
    slot_arena: SlotArena,
    clock: C,
    liveness_policy: crate::runtime::config::LivenessPolicy,
    _epoch_marker: PhantomData<E>,
}

/// Affine bundle exposing slot storage access.
#[cfg(test)]
pub(crate) struct SlotBundle<'rv> {
    arena: &'rv mut SlotArena,
}

#[cfg(test)]
impl<'rv> SlotBundle<'rv> {
    #[inline]
    fn new(arena: &'rv mut SlotArena) -> Self {
        Self { arena }
    }

    /// Borrow the underlying slot arena.
    #[inline]
    pub(crate) fn arena(&mut self) -> &mut SlotArena {
        self.arena
    }
}

#[derive(Clone, Copy, Debug)]
struct EffectContext {
    sid: SessionId,
    lane: Lane,
    generation: Option<Generation>,
    fences: Option<(u32, u32)>,
    expected_topology_ack: Option<TopologyAck>,
    delegate: Option<DelegateContext>,
}

impl EffectContext {
    fn new(sid: SessionId, lane: Lane) -> Self {
        Self {
            sid,
            lane,
            generation: None,
            fences: None,
            expected_topology_ack: None,
            delegate: None,
        }
    }

    fn with_generation(mut self, generation: Generation) -> Self {
        self.generation = Some(generation);
        self
    }

    fn with_fences(mut self, fences: Option<(u32, u32)>) -> Self {
        self.fences = fences;
        self
    }

    fn with_expected_topology_ack(mut self, expected_topology_ack: Option<TopologyAck>) -> Self {
        self.expected_topology_ack = expected_topology_ack;
        self
    }

    fn with_delegate(mut self, delegate: DelegateContext) -> Self {
        self.delegate = Some(delegate);
        self
    }
}

enum EffectResult {
    None,
    Generation(Generation),
}

#[derive(Debug)]
enum EffectError {
    StateRestore(StateRestoreError),
    TxAbort(TxAbortError),
    TxCommit(super::error::TxCommitError),
    MissingGeneration,
    Unsupported,
    Topology(TopologyError),
    Delegation(super::error::CapError),
}

#[derive(Clone, Copy, Debug)]
struct DelegateContext {
    claim: bool,
    token: GenericCapToken<EndpointResource>,
}

impl<'rv, 'cfg, T: Transport, U: LabelUniverse, C: Clock, E: crate::control::cap::mint::EpochTable>
    Rendezvous<'rv, 'cfg, T, U, C, E>
where
    'cfg: 'rv,
{
    #[inline(always)]
    const fn align_up(value: usize, align: usize) -> usize {
        let mask = align.saturating_sub(1);
        (value + mask) & !mask
    }

    #[inline(always)]
    const fn frontier_scratch_guard_bytes(
        layout: crate::endpoint::kernel::FrontierScratchLayout,
    ) -> usize {
        layout
            .total_bytes()
            .saturating_add(layout.total_align().saturating_sub(1))
    }

    #[inline(always)]
    const fn align_down(value: usize, align: usize) -> usize {
        let mask = align.saturating_sub(1);
        value & !mask
    }

    #[inline(always)]
    pub(crate) fn program_image_guard_bytes(&self) -> usize {
        let (slab_ptr, _) = self.slab_ptr_and_len();
        let base = slab_ptr as usize;
        let start = Self::align_up(
            base + self.endpoint_lease_floor(),
            CompiledProgramFacts::persistent_align(),
        )
        .saturating_sub(base);
        start + CompiledProgramFacts::max_persistent_bytes() - self.endpoint_lease_floor()
    }

    #[inline(always)]
    pub(crate) fn role_image_guard_bytes(&self, bytes: usize) -> usize {
        let (slab_ptr, _) = self.slab_ptr_and_len();
        let base = slab_ptr as usize;
        let start = Self::align_up(
            base + self.endpoint_lease_floor(),
            CompiledRoleImage::persistent_align(),
        )
        .saturating_sub(base);
        start + bytes - self.endpoint_lease_floor()
    }

    #[inline(always)]
    pub(crate) fn program_and_role_image_guard_bytes(&self, role_image_bytes: usize) -> usize {
        let (slab_ptr, _) = self.slab_ptr_and_len();
        let base = slab_ptr as usize;
        let program_end = Self::align_up(
            base + self.endpoint_lease_floor(),
            CompiledProgramFacts::persistent_align(),
        ) + CompiledProgramFacts::max_persistent_bytes();
        let role_end =
            Self::align_up(program_end, CompiledRoleImage::persistent_align()) + role_image_bytes;
        role_end - self.endpoint_lease_floor() - base
    }

    #[inline]
    pub(crate) fn slab_ptr_and_len(&self) -> (*mut u8, usize) {
        unsafe {
            let slab = &mut *self.slab;
            (slab.as_mut_ptr(), slab.len())
        }
    }

    #[inline]
    fn endpoint_storage_floor(&self) -> usize {
        let (_, slab_len) = self.slab_ptr_and_len();
        let mut floor = slab_len;
        let mut idx = 0usize;
        while idx < usize::from(self.endpoint_lease_capacity) {
            let slot = unsafe { &*self.endpoint_leases.add(idx) };
            if slot.occupied && slot.len != 0 && (slot.offset as usize) < floor {
                floor = slot.offset as usize;
            }
            idx += 1;
        }
        floor
    }

    #[inline]
    pub(crate) fn scratch_storage_ptr_and_len(&self) -> (*mut u8, usize) {
        let (ptr, _) = self.slab_ptr_and_len();
        let start = self.endpoint_lease_floor();
        let end = self.endpoint_storage_floor();
        let len = end.saturating_sub(start);
        unsafe { (ptr.add(start), len) }
    }

    #[inline]
    fn endpoint_lease_floor(&self) -> usize {
        self.image_frontier as usize + self.scratch_reserved_bytes as usize
    }

    #[cfg(test)]
    #[inline]
    fn update_runtime_frontier(&mut self) {
        let frontier = self
            .image_frontier
            .saturating_add(self.scratch_reserved_bytes);
        if frontier > self.runtime_frontier {
            self.runtime_frontier = frontier;
        }
    }

    #[inline]
    fn set_image_frontier(&mut self, frontier: u32) {
        self.image_frontier = frontier;
        #[cfg(test)]
        self.update_runtime_frontier();
    }

    #[inline]
    fn set_scratch_reserved_bytes(&mut self, reserved: u32) {
        self.scratch_reserved_bytes = reserved;
        #[cfg(test)]
        self.update_runtime_frontier();
    }

    #[inline]
    fn reserve_scratch_reserved_bytes(&mut self, reserved: u32) {
        if reserved > self.scratch_reserved_bytes {
            self.scratch_reserved_bytes = reserved;
            #[cfg(test)]
            self.update_runtime_frontier();
        }
    }

    #[cfg(all(test, feature = "std"))]
    #[inline]
    pub(crate) fn runtime_sidecar_high_water_bytes(&self) -> usize {
        self.runtime_frontier as usize
    }

    #[cfg(all(test, feature = "std"))]
    #[inline]
    pub(crate) fn live_endpoint_storage_bytes(&self) -> usize {
        let mut bytes = 0usize;
        let mut idx = 0usize;
        while idx < usize::from(self.endpoint_lease_capacity) {
            let slot = unsafe { &*self.endpoint_leases.add(idx) };
            if slot.occupied {
                bytes = bytes.saturating_add(slot.len as usize);
            }
            idx += 1;
        }
        bytes
    }

    #[inline]
    fn free_region_empty_slots(&self) -> usize {
        let mut empty = 0usize;
        let mut idx = 0usize;
        while idx < FREE_REGION_CAPACITY {
            if !self.free_regions[idx].occupied {
                empty += 1;
            }
            idx += 1;
        }
        empty
    }

    #[inline]
    fn first_empty_free_region_slot(&self) -> Option<usize> {
        let mut idx = 0usize;
        while idx < FREE_REGION_CAPACITY {
            if !self.free_regions[idx].occupied {
                return Some(idx);
            }
            idx += 1;
        }
        None
    }

    #[inline]
    fn clear_free_region(&mut self, idx: usize) {
        if idx < FREE_REGION_CAPACITY {
            self.free_regions[idx] = FreeRegion::EMPTY;
        }
    }

    fn release_persistent_region(&mut self, offset: u32, len: u32) {
        if len == 0 {
            return;
        }
        let mut start = offset;
        let mut end = offset.saturating_add(len);
        let mut idx = 0usize;
        while idx < FREE_REGION_CAPACITY {
            let region = self.free_regions[idx];
            if !region.occupied {
                idx += 1;
                continue;
            }
            let region_start = region.offset;
            let region_end = region.offset.saturating_add(region.len);
            if region_end < start || region_start > end {
                idx += 1;
                continue;
            }
            start = core::cmp::min(start, region_start);
            end = core::cmp::max(end, region_end);
            self.clear_free_region(idx);
            idx = 0;
        }

        if end == self.image_frontier {
            self.set_image_frontier(start);
            loop {
                let mut trimmed = false;
                let mut free_idx = 0usize;
                while free_idx < FREE_REGION_CAPACITY {
                    let region = self.free_regions[free_idx];
                    if region.occupied
                        && region.offset.saturating_add(region.len) == self.image_frontier
                    {
                        self.set_image_frontier(region.offset);
                        self.clear_free_region(free_idx);
                        trimmed = true;
                        break;
                    }
                    free_idx += 1;
                }
                if !trimmed {
                    break;
                }
            }
            return;
        }

        if let Some(idx) = self.first_empty_free_region_slot() {
            self.free_regions[idx] = FreeRegion {
                offset: start,
                len: end.saturating_sub(start),
                occupied: true,
            };
        }
    }

    unsafe fn allocate_from_free_regions(
        &mut self,
        bytes: usize,
        align: usize,
    ) -> Option<(*mut u8, u32)> {
        let (slab_ptr, _) = self.slab_ptr_and_len();
        let base = slab_ptr as usize;
        let mut idx = 0usize;
        while idx < FREE_REGION_CAPACITY {
            let region = self.free_regions[idx];
            if !region.occupied {
                idx += 1;
                continue;
            }
            let region_start = region.offset as usize;
            let region_end = region.offset as usize + region.len as usize;
            let alloc_start = Self::align_up(base + region_start, align).checked_sub(base)?;
            let alloc_end = alloc_start.checked_add(bytes)?;
            if alloc_end > region_end {
                idx += 1;
                continue;
            }
            let prefix_len = alloc_start.saturating_sub(region_start);
            let suffix_len = region_end.saturating_sub(alloc_end);
            let fragments = usize::from(prefix_len != 0) + usize::from(suffix_len != 0);
            if self.free_region_empty_slots() + 1 < fragments {
                idx += 1;
                continue;
            }
            self.clear_free_region(idx);
            if prefix_len != 0 {
                self.release_persistent_region(region.offset, prefix_len as u32);
            }
            if suffix_len != 0 {
                self.release_persistent_region(alloc_end as u32, suffix_len as u32);
            }
            return Some((unsafe { slab_ptr.add(alloc_start) }, alloc_start as u32));
        }
        None
    }

    #[inline]
    unsafe fn allocate_persistent_sidecar_bytes(
        &mut self,
        bytes: usize,
        align: usize,
    ) -> Option<(*mut u8, u32)> {
        if let Some(region) = unsafe { self.allocate_from_free_regions(bytes, align) } {
            return Some(region);
        }
        let (slab_ptr, _) = self.slab_ptr_and_len();
        let base = slab_ptr as usize;
        let start = Self::align_up(base + self.image_frontier as usize, align).checked_sub(base)?;
        let end = start.checked_add(bytes)?;
        if end > self.endpoint_storage_floor() {
            return None;
        }
        self.set_image_frontier(end as u32);
        Some((unsafe { slab_ptr.add(start) }, start as u32))
    }

    #[inline]
    unsafe fn allocate_persistent_image_bytes(
        &mut self,
        bytes: usize,
        align: usize,
    ) -> Option<(*mut u8, u32)> {
        unsafe { self.allocate_persistent_sidecar_bytes(bytes, align) }
    }

    #[inline]
    pub(crate) fn allocate_external_persistent_sidecar_bytes(
        &mut self,
        bytes: usize,
        align: usize,
    ) -> Option<(*mut u8, usize)> {
        let prior_frontier = self.image_frontier;
        let (ptr, offset) = unsafe { self.allocate_persistent_sidecar_bytes(bytes, align) }?;
        let reclaim_delta = if offset > prior_frontier {
            offset.saturating_sub(prior_frontier) as usize
        } else {
            0
        };
        Some((ptr, reclaim_delta))
    }

    #[inline]
    fn reclaim_offset_for_payload(&self, ptr: *mut u8, reclaim_delta: usize) -> u32 {
        let (slab_ptr, _) = self.slab_ptr_and_len();
        let base = slab_ptr.addr();
        let payload_start = ptr.addr().saturating_sub(base);
        let reclaim_start = payload_start.checked_sub(reclaim_delta).unwrap();
        u32::try_from(reclaim_start).unwrap()
    }

    #[inline]
    fn free_bound_persistent_region(&mut self, reclaim_offset: u32, ptr: *mut u8, bytes: usize) {
        if ptr.is_null() || bytes == 0 {
            return;
        }
        let (slab_ptr, _) = self.slab_ptr_and_len();
        let base = slab_ptr.addr();
        let payload_start = ptr.addr().saturating_sub(base);
        let reclaim_start = reclaim_offset as usize;
        let payload_end = payload_start.checked_add(bytes).unwrap();
        let release_len = payload_end.checked_sub(reclaim_start).unwrap();
        let release_len = u32::try_from(release_len).unwrap();
        self.release_persistent_region(reclaim_offset, release_len);
    }

    #[inline]
    fn first_free_program_image_slot(&self) -> Option<usize> {
        let mut idx = 0usize;
        while idx < self.image_slot_capacity as usize {
            let slot = unsafe { &*self.program_images.add(idx) };
            if !slot.occupied {
                return Some(idx);
            }
            idx += 1;
        }
        None
    }

    #[inline]
    fn first_reusable_program_image_slot(&self, min_len: usize) -> Option<usize> {
        let mut idx = 0usize;
        while idx < self.image_slot_capacity as usize {
            let slot = unsafe { &*self.program_images.add(idx) };
            if slot.occupied && slot.pins == 0 && slot.len as usize >= min_len {
                return Some(idx);
            }
            idx += 1;
        }
        None
    }

    #[inline]
    fn first_unpinned_program_image_slot(&self) -> Option<usize> {
        let mut idx = 0usize;
        while idx < self.image_slot_capacity as usize {
            let slot = unsafe { &*self.program_images.add(idx) };
            if slot.occupied && slot.pins == 0 {
                return Some(idx);
            }
            idx += 1;
        }
        None
    }

    #[inline]
    fn program_image_slot_index(&self, stamp: ProgramStamp) -> Option<usize> {
        let mut idx = 0usize;
        while idx < self.image_slot_capacity as usize {
            let slot = unsafe { &*self.program_images.add(idx) };
            if slot.occupied && slot.stamp == stamp {
                return Some(idx);
            }
            idx += 1;
        }
        None
    }

    #[inline]
    unsafe fn program_image_from_slot(
        &self,
        slot: &CompiledProgramSlot,
    ) -> *const CompiledProgramFacts {
        let (slab_ptr, _) = self.slab_ptr_and_len();
        unsafe {
            slab_ptr
                .add(slot.offset as usize)
                .cast::<CompiledProgramFacts>()
        }
    }

    #[inline]
    pub(crate) fn free_external_persistent_sidecar_bytes(
        &mut self,
        ptr: *mut u8,
        bytes: usize,
        reclaim_delta: usize,
    ) {
        if ptr.is_null() || bytes == 0 {
            return;
        }
        let reclaim_offset = self.reclaim_offset_for_payload(ptr, reclaim_delta);
        self.free_bound_persistent_region(reclaim_offset, ptr, bytes);
    }

    #[inline]
    fn pin_program_image(&mut self, stamp: ProgramStamp) -> Option<u8> {
        let idx = self.program_image_slot_index(stamp)?;
        let slot = unsafe { &mut *self.program_images.add(idx) };
        slot.pins = slot.pins.saturating_add(1);
        Some(idx as u8)
    }

    #[inline]
    fn unpin_program_image_slot(&mut self, idx: usize) {
        if idx >= self.image_slot_capacity as usize {
            return;
        }
        let slot = unsafe { &mut *self.program_images.add(idx) };
        if slot.occupied && slot.pins != 0 {
            slot.pins -= 1;
        }
    }

    #[inline]
    fn endpoint_lease(
        &self,
        lease_slot: EndpointLeaseId,
        generation: u32,
    ) -> Option<&EndpointLeaseSlot> {
        let idx = usize::from(lease_slot);
        if idx >= usize::from(self.endpoint_lease_capacity) {
            return None;
        }
        let slot = unsafe { &*self.endpoint_leases.add(idx) };
        if slot.occupied && slot.generation == generation {
            Some(slot)
        } else {
            None
        }
    }

    #[inline]
    fn endpoint_lease_mut(
        &mut self,
        lease_slot: EndpointLeaseId,
        generation: u32,
    ) -> Option<&mut EndpointLeaseSlot> {
        let idx = usize::from(lease_slot);
        if idx >= usize::from(self.endpoint_lease_capacity) {
            return None;
        }
        let slot = unsafe { &mut *self.endpoint_leases.add(idx) };
        if slot.occupied && slot.generation == generation {
            Some(slot)
        } else {
            None
        }
    }

    #[inline]
    pub(crate) const fn endpoint_lease_capacity(&self) -> EndpointLeaseId {
        self.endpoint_lease_capacity
    }

    #[inline]
    fn next_endpoint_lease_generation(slot: &mut EndpointLeaseSlot) -> u32 {
        let next = slot.generation.wrapping_add(1);
        if next == 0 { 1 } else { next }
    }

    #[inline]
    pub(crate) fn endpoint_lease_storage(
        &self,
        lease_slot: EndpointLeaseId,
        generation: u32,
    ) -> Option<(usize, usize)> {
        let slot = self.endpoint_lease(lease_slot, generation)?;
        Some((slot.offset as usize, slot.len as usize))
    }

    #[inline]
    pub(crate) fn public_endpoint_lease_by_index(
        &self,
        idx: usize,
    ) -> Option<(EndpointLeaseId, u32)> {
        if idx >= usize::from(self.endpoint_lease_capacity) {
            return None;
        }
        let slot = unsafe { &*self.endpoint_leases.add(idx) };
        if !slot.occupied || !slot.public_endpoint {
            return None;
        }
        Some((EndpointLeaseId::try_from(idx).ok()?, slot.generation))
    }

    #[inline]
    fn resident_route_frame_slots_floor(&self) -> usize {
        let mut required = 0usize;
        let mut idx = 0usize;
        while idx < usize::from(self.endpoint_lease_capacity) {
            let slot = unsafe { &*self.endpoint_leases.add(idx) };
            if slot.occupied {
                required =
                    core::cmp::max(required, slot.resident_budget.route_frame_slots as usize);
            }
            idx += 1;
        }
        required
    }

    #[inline]
    fn resident_route_lane_slots_floor(&self) -> usize {
        let mut required = 0usize;
        let mut idx = 0usize;
        while idx < usize::from(self.endpoint_lease_capacity) {
            let slot = unsafe { &*self.endpoint_leases.add(idx) };
            if slot.occupied {
                required = core::cmp::max(required, slot.resident_budget.route_lane_slots as usize);
            }
            idx += 1;
        }
        required
    }

    fn resident_loop_slots_floor(&self) -> usize {
        let mut required = 0usize;
        let mut idx = 0usize;
        while idx < usize::from(self.endpoint_lease_capacity) {
            let slot = unsafe { &*self.endpoint_leases.add(idx) };
            if slot.occupied {
                required = core::cmp::max(required, slot.resident_budget.loop_slots as usize);
            }
            idx += 1;
        }
        required
    }

    #[inline]
    fn resident_cap_entries_floor(&self) -> usize {
        let mut required = self.caps.live_count();
        let mut idx = 0usize;
        while idx < usize::from(self.endpoint_lease_capacity) {
            let slot = unsafe { &*self.endpoint_leases.add(idx) };
            if slot.occupied {
                required = required.saturating_add(slot.resident_budget.cap_entries as usize);
            }
            idx += 1;
        }
        required
    }

    #[inline]
    fn lane_base(&self) -> u32 {
        self.lane_range.start
    }

    #[inline]
    fn lane_slot_count(&self) -> usize {
        self.lane_range.end.saturating_sub(self.lane_range.start) as usize
    }

    fn recompute_scratch_reserved_bytes(&mut self) {
        let (slab_ptr, _) = self.slab_ptr_and_len();
        let mut reserved = 0u32;
        let mut idx = 0usize;
        while idx < self.image_slot_capacity as usize {
            let slot = unsafe { &*self.role_images.add(idx) };
            if slot.occupied {
                let role_ptr = unsafe {
                    slab_ptr
                        .add(slot.offset as usize)
                        .cast::<CompiledRoleImage>()
                };
                let bytes = Self::frontier_scratch_guard_bytes(unsafe {
                    (*role_ptr).frontier_scratch_layout()
                }) as u32;
                if bytes > reserved {
                    reserved = bytes;
                }
            }
            idx += 1;
        }
        self.set_scratch_reserved_bytes(reserved);
    }

    fn ensure_route_table_capacity(
        &mut self,
        required_frame_slots: usize,
        required_lane_slots: usize,
    ) -> Option<()> {
        let required_lane_slots = core::cmp::max(required_lane_slots, self.lane_slot_count());
        if required_frame_slots == 0
            || (self.routes.route_slots() >= required_frame_slots
                && self.routes.lane_slots() >= required_lane_slots)
        {
            return Some(());
        }
        let prior_frontier = self.image_frontier;
        let (storage, storage_offset) = unsafe {
            self.allocate_persistent_sidecar_bytes(
                RouteTable::storage_bytes(required_frame_slots, required_lane_slots),
                RouteTable::storage_align(),
            )
        }?;
        let reclaim_delta = storage_offset.saturating_sub(prior_frontier) as usize;
        let old_ptr = self.routes.storage_ptr();
        let old_bytes = self.routes.storage_bytes_current();
        let old_reclaim_offset =
            self.reclaim_offset_for_payload(old_ptr, self.routes.storage_reclaim_delta());
        if self.routes.route_slots() == 0 {
            unsafe {
                self.routes.bind_from_storage_with_layout(
                    storage,
                    required_frame_slots,
                    self.lane_base(),
                    required_lane_slots,
                    reclaim_delta,
                );
            }
        } else {
            unsafe {
                self.routes.migrate_from_storage(
                    storage,
                    required_frame_slots,
                    self.lane_base(),
                    required_lane_slots,
                );
                self.routes.rebind_from_storage(
                    storage,
                    required_frame_slots,
                    self.lane_base(),
                    required_lane_slots,
                    reclaim_delta,
                );
            }
            self.free_bound_persistent_region(old_reclaim_offset, old_ptr, old_bytes);
        }
        Some(())
    }

    fn ensure_loop_table_capacity(&mut self, required_slots: usize) -> Option<()> {
        let required_lane_slots = self.lane_slot_count();
        if required_slots == 0 || self.loops.loop_slots() >= required_slots {
            return Some(());
        }
        let prior_frontier = self.image_frontier;
        let (storage, storage_offset) = unsafe {
            self.allocate_persistent_sidecar_bytes(
                LoopTable::storage_bytes(required_slots, required_lane_slots),
                LoopTable::storage_align(),
            )
        }?;
        let reclaim_delta = storage_offset.saturating_sub(prior_frontier) as usize;
        let old_ptr = self.loops.storage_ptr();
        let old_bytes = self.loops.storage_bytes_current();
        let old_reclaim_offset =
            self.reclaim_offset_for_payload(old_ptr, self.loops.storage_reclaim_delta());
        if self.loops.loop_slots() == 0 {
            unsafe {
                self.loops.bind_from_storage(
                    storage,
                    required_slots,
                    self.lane_base(),
                    required_lane_slots,
                    reclaim_delta,
                );
            }
        } else {
            unsafe {
                self.loops.migrate_from_storage(
                    storage,
                    required_slots,
                    self.lane_base(),
                    required_lane_slots,
                );
                self.loops.rebind_from_storage(
                    storage,
                    required_slots,
                    self.lane_base(),
                    required_lane_slots,
                    reclaim_delta,
                );
            }
            self.free_bound_persistent_region(old_reclaim_offset, old_ptr, old_bytes);
        }
        Some(())
    }

    fn ensure_generation_table_storage(&mut self) -> Option<()> {
        if self.r#gen.is_bound() || self.lane_slot_count() == 0 {
            return Some(());
        }
        let lane_slots = self.lane_slot_count();
        let (storage, _) = unsafe {
            self.allocate_persistent_sidecar_bytes(
                GenTable::storage_bytes(lane_slots),
                GenTable::storage_align(),
            )
        }?;
        unsafe {
            self.r#gen
                .bind_from_storage(storage, self.lane_base(), lane_slots);
        }
        Some(())
    }

    fn ensure_assoc_table_storage(&mut self) -> Option<()> {
        if self.assoc.is_bound() || self.lane_slot_count() == 0 {
            return Some(());
        }
        let lane_slots = self.lane_slot_count();
        let (storage, _) = unsafe {
            self.allocate_persistent_sidecar_bytes(
                AssocTable::storage_bytes(lane_slots),
                AssocTable::storage_align(),
            )
        }?;
        unsafe {
            self.assoc
                .bind_from_storage(storage, self.lane_base(), lane_slots);
        }
        Some(())
    }

    fn ensure_checkpoint_table_storage(&mut self) -> Option<()> {
        if self.state_snapshots.is_bound() || self.lane_slot_count() == 0 {
            return Some(());
        }
        let lane_slots = self.lane_slot_count();
        let (storage, _) = unsafe {
            self.allocate_persistent_sidecar_bytes(
                StateSnapshotTable::storage_bytes(lane_slots),
                StateSnapshotTable::storage_align(),
            )
        }?;
        unsafe {
            self.state_snapshots
                .bind_from_storage(storage, self.lane_base(), lane_slots);
        }
        Some(())
    }

    fn ensure_cap_table_capacity(&mut self, required_entries: usize) -> Option<()> {
        if required_entries == 0 || self.caps.capacity() >= required_entries {
            return Some(());
        }
        let prior_frontier = self.image_frontier;
        let (storage, storage_offset) = unsafe {
            self.allocate_persistent_sidecar_bytes(
                CapTable::storage_bytes(required_entries),
                CapTable::storage_align(),
            )
        }?;
        let reclaim_delta = storage_offset.saturating_sub(prior_frontier) as usize;
        let old_ptr = self.caps.storage_ptr();
        let old_bytes = self.caps.storage_bytes_current();
        let old_reclaim_offset =
            self.reclaim_offset_for_payload(old_ptr, self.caps.storage_reclaim_delta());
        if self.caps.capacity() == 0 {
            unsafe {
                self.caps
                    .bind_from_storage(storage, required_entries, reclaim_delta);
            }
        } else {
            let migrated = unsafe { self.caps.migrate_from_storage(storage, required_entries) };
            if !migrated {
                self.free_bound_persistent_region(
                    storage_offset.saturating_sub(reclaim_delta as u32),
                    storage,
                    CapTable::storage_bytes(required_entries),
                );
                return None;
            }
            unsafe {
                self.caps
                    .rebind_from_storage(storage, required_entries, reclaim_delta);
            }
            self.free_bound_persistent_region(old_reclaim_offset, old_ptr, old_bytes);
        }
        Some(())
    }

    fn ensure_topology_table_storage(&mut self) -> Option<()> {
        if self.topology.is_bound() {
            return Some(());
        }
        let lane_slots = self.lane_slot_count();
        let (storage, _) = unsafe {
            self.allocate_persistent_sidecar_bytes(
                TopologyStateTable::storage_bytes(lane_slots),
                TopologyStateTable::storage_align(),
            )
        }?;
        unsafe {
            self.topology
                .bind_from_storage(storage, self.lane_base(), lane_slots);
        }
        Some(())
    }

    pub(crate) fn ensure_topology_control_storage(&mut self) -> Option<()> {
        self.ensure_topology_table_storage()
    }

    pub(crate) fn prepare_topology_control_scope(&mut self, lane: Lane) -> Option<()> {
        self.ensure_topology_control_storage()?;
        self.initialise_control_scope(lane, ControlScopeKind::Topology);
        Some(())
    }

    fn ensure_policy_table_storage(&mut self) -> Option<()> {
        if self.policies.is_bound() {
            return Some(());
        }
        let lane_slots = self.lane_slot_count();
        let (storage, _) = unsafe {
            self.allocate_persistent_sidecar_bytes(
                PolicyTable::storage_bytes(lane_slots),
                PolicyTable::storage_align(),
            )
        }?;
        unsafe {
            self.policies
                .bind_from_storage(storage, self.lane_base(), lane_slots);
        }
        Some(())
    }

    fn ensure_core_lane_storage(&mut self) -> Option<()> {
        self.ensure_generation_table_storage()?;
        self.ensure_assoc_table_storage()?;
        self.ensure_checkpoint_table_storage()?;
        self.ensure_policy_table_storage()?;
        Some(())
    }

    fn free_bound_core_lane_storage(&mut self) {
        if self.policies.is_bound() {
            self.free_external_persistent_sidecar_bytes(
                self.policies.storage_ptr(),
                self.policies.storage_bytes_current(),
                0,
            );
            self.policies = PolicyTable::empty();
        }
        if self.state_snapshots.is_bound() {
            self.free_external_persistent_sidecar_bytes(
                self.state_snapshots.storage_ptr(),
                self.state_snapshots.storage_bytes_current(),
                0,
            );
            self.state_snapshots = StateSnapshotTable::empty();
        }
        if self.assoc.is_bound() {
            self.free_external_persistent_sidecar_bytes(
                self.assoc.storage_ptr(),
                self.assoc.storage_bytes_current(),
                0,
            );
            self.assoc = AssocTable::empty();
        }
        if self.r#gen.is_bound() {
            self.free_external_persistent_sidecar_bytes(
                self.r#gen.storage_ptr(),
                self.r#gen.storage_bytes_current(),
                0,
            );
            self.r#gen = GenTable::empty();
        }
    }

    unsafe fn cleanup_failed_public_init(dst: *mut Self) {
        let rv = unsafe { &mut *dst };
        rv.free_bound_core_lane_storage();
        unsafe {
            core::ptr::drop_in_place(core::ptr::addr_of_mut!((*dst).clock));
            core::ptr::drop_in_place(core::ptr::addr_of_mut!((*dst).transport));
        }
    }

    #[cfg(test)]
    fn ensure_slot_arena_storage(&mut self) -> Option<()> {
        if !self.slot_arena.slots_ptr().is_null() {
            return Some(());
        }
        let (storage, _) = unsafe {
            self.allocate_persistent_sidecar_bytes(
                SlotArena::storage_bytes(),
                SlotArena::storage_align(),
            )
        }?;
        unsafe {
            self.slot_arena.bind_from_storage(storage);
        }
        Some(())
    }

    pub(crate) fn ensure_endpoint_resident_budget(
        &mut self,
        budget: EndpointResidentBudget,
    ) -> Option<()> {
        let route_frame_slots = core::cmp::max(
            self.resident_route_frame_slots_floor(),
            budget.route_frame_slots as usize,
        );
        let route_lane_slots = core::cmp::max(
            self.resident_route_lane_slots_floor(),
            budget.route_lane_slots as usize,
        );
        let loop_slots =
            core::cmp::max(self.resident_loop_slots_floor(), budget.loop_slots as usize);
        let cap_entries = core::cmp::max(
            self.resident_cap_entries_floor(),
            budget.cap_entries as usize,
        );
        self.ensure_route_table_capacity(route_frame_slots, route_lane_slots)?;
        self.ensure_loop_table_capacity(loop_slots)?;
        self.ensure_cap_table_capacity(cap_entries)?;
        Some(())
    }

    fn trim_resident_headers_to_live_budget(&mut self) {
        if self.resident_route_frame_slots_floor() == 0 && self.routes.route_slots() != 0 {
            self.free_bound_persistent_region(
                self.reclaim_offset_for_payload(
                    self.routes.storage_ptr(),
                    self.routes.storage_reclaim_delta(),
                ),
                self.routes.storage_ptr(),
                self.routes.storage_bytes_current(),
            );
            self.routes = RouteTable::empty();
        }
        if self.resident_loop_slots_floor() == 0 && self.loops.loop_slots() != 0 {
            self.free_bound_persistent_region(
                self.reclaim_offset_for_payload(
                    self.loops.storage_ptr(),
                    self.loops.storage_reclaim_delta(),
                ),
                self.loops.storage_ptr(),
                self.loops.storage_bytes_current(),
            );
            self.loops = LoopTable::empty();
        }
        if self.resident_cap_entries_floor() == 0 && self.caps.capacity() != 0 {
            self.free_bound_persistent_region(
                self.reclaim_offset_for_payload(
                    self.caps.storage_ptr(),
                    self.caps.storage_reclaim_delta(),
                ),
                self.caps.storage_ptr(),
                self.caps.storage_bytes_current(),
            );
            self.caps = CapTable::empty();
        }
    }

    #[inline]
    fn first_free_role_image_slot(&self) -> Option<usize> {
        let mut idx = 0usize;
        while idx < self.image_slot_capacity as usize {
            let slot = unsafe { &*self.role_images.add(idx) };
            if !slot.occupied {
                return Some(idx);
            }
            idx += 1;
        }
        None
    }

    #[inline]
    fn first_reusable_role_image_slot(&self, min_len: usize) -> Option<usize> {
        let mut idx = 0usize;
        while idx < self.image_slot_capacity as usize {
            let slot = unsafe { &*self.role_images.add(idx) };
            if slot.occupied && slot.pins == 0 && slot.len as usize >= min_len {
                return Some(idx);
            }
            idx += 1;
        }
        None
    }

    #[inline]
    fn first_unpinned_role_image_slot(&self) -> Option<usize> {
        let mut idx = 0usize;
        while idx < self.image_slot_capacity as usize {
            let slot = unsafe { &*self.role_images.add(idx) };
            if slot.occupied && slot.pins == 0 {
                return Some(idx);
            }
            idx += 1;
        }
        None
    }

    #[inline]
    fn role_image_slot_index_for(&self, stamp: ProgramStamp, role: u8) -> Option<usize> {
        let mut idx = 0usize;
        while idx < self.image_slot_capacity as usize {
            let slot = unsafe { &*self.role_images.add(idx) };
            if slot.occupied && slot.stamp == stamp && slot.role == role {
                return Some(idx);
            }
            idx += 1;
        }
        None
    }

    fn pin_role_image_for(&mut self, stamp: ProgramStamp, role: u8) -> Option<u8> {
        let idx = self.role_image_slot_index_for(stamp, role)?;
        let slot = unsafe { &mut *self.role_images.add(idx) };
        slot.pins = slot.pins.saturating_add(1);
        Some(idx as u8)
    }

    #[inline]
    fn unpin_role_image_slot(&mut self, idx: usize) {
        if idx >= self.image_slot_capacity as usize {
            return;
        }
        let slot = unsafe { &mut *self.role_images.add(idx) };
        if slot.occupied && slot.pins != 0 {
            slot.pins -= 1;
        }
    }

    #[inline]
    pub(crate) fn pin_endpoint_images<const ROLE: u8>(
        &mut self,
        lease_slot: EndpointLeaseId,
        generation: u32,
        stamp: ProgramStamp,
    ) -> bool {
        let Some(program_image_slot) = self.pin_program_image(stamp) else {
            return false;
        };
        let Some(role_image_slot) = self.pin_role_image_for(stamp, ROLE) else {
            self.unpin_program_image_slot(program_image_slot as usize);
            return false;
        };
        if !self.record_endpoint_image_slots(
            lease_slot,
            generation,
            program_image_slot,
            role_image_slot,
        ) {
            self.unpin_program_image_slot(program_image_slot as usize);
            self.unpin_role_image_slot(role_image_slot as usize);
            return false;
        }
        true
    }

    #[inline]
    pub(crate) fn record_endpoint_image_slots(
        &mut self,
        lease_slot: EndpointLeaseId,
        generation: u32,
        program_image_slot: u8,
        role_image_slot: u8,
    ) -> bool {
        if let Some(slot) = self.endpoint_lease_mut(lease_slot, generation) {
            slot.program_image_slot = program_image_slot;
            slot.role_image_slot = role_image_slot;
            return true;
        }
        false
    }

    #[inline]
    pub(crate) fn mark_public_endpoint_lease(
        &mut self,
        lease_slot: EndpointLeaseId,
        generation: u32,
    ) -> bool {
        if let Some(slot) = self.endpoint_lease_mut(lease_slot, generation) {
            slot.public_endpoint = true;
            return true;
        }
        false
    }

    #[inline(never)]
    pub(crate) unsafe fn materialize_program_image_from_summary(
        &mut self,
        stamp: ProgramStamp,
        summary: &LoweringSummary,
    ) -> Option<*const CompiledProgramFacts> {
        if let Some(idx) = self.program_image_slot_index(stamp) {
            let slot = unsafe { &*self.program_images.add(idx) };
            return Some(unsafe { self.program_image_from_slot(slot) });
        }
        let Some(insert_idx) = self.first_free_program_image_slot() else {
            return unsafe { self.recycle_program_image_from_summary(stamp, summary) };
        };
        let counts = summary.compiled_program_counts();
        let bytes = CompiledProgramFacts::persistent_bytes_for_counts(counts);
        let (ptr, offset) = unsafe {
            self.allocate_persistent_image_bytes(bytes, CompiledProgramFacts::persistent_align())
        }?;
        unsafe {
            crate::global::compiled::materialize::init_compiled_program_image_from_summary(
                ptr.cast::<CompiledProgramFacts>(),
                summary,
            );
        }
        let slot = unsafe { &mut *self.program_images.add(insert_idx) };
        *slot = CompiledProgramSlot {
            stamp,
            offset,
            len: bytes as u32,
            pins: 0,
            occupied: true,
        };
        Some(ptr.cast::<CompiledProgramFacts>())
    }

    #[cold]
    #[inline(never)]
    unsafe fn recycle_program_image_from_summary(
        &mut self,
        stamp: ProgramStamp,
        summary: &LoweringSummary,
    ) -> Option<*const CompiledProgramFacts> {
        let counts = summary.compiled_program_counts();
        let bytes = CompiledProgramFacts::persistent_bytes_for_counts(counts);
        if let Some(insert_idx) = self.first_reusable_program_image_slot(bytes) {
            let slot = unsafe { &mut *self.program_images.add(insert_idx) };
            let ptr = unsafe {
                self.slab_ptr_and_len()
                    .0
                    .add(slot.offset as usize)
                    .cast::<CompiledProgramFacts>()
            };
            unsafe {
                crate::global::compiled::materialize::init_compiled_program_image_from_summary(
                    ptr, summary,
                );
            }
            slot.stamp = stamp;
            slot.occupied = true;
            debug_assert_eq!(slot.pins, 0);
            return Some(ptr.cast_const());
        }
        let insert_idx = self.first_unpinned_program_image_slot()?;
        let (ptr, offset, reserved_len, released_region) = {
            let slot = unsafe { &*self.program_images.add(insert_idx) };
            let offset = slot.offset;
            if slot.len as usize >= bytes {
                let ptr = unsafe {
                    self.slab_ptr_and_len()
                        .0
                        .add(offset as usize)
                        .cast::<CompiledProgramFacts>()
                };
                (ptr.cast::<u8>(), offset, slot.len, None)
            } else {
                let (ptr, offset) = unsafe {
                    self.allocate_persistent_image_bytes(
                        bytes,
                        CompiledProgramFacts::persistent_align(),
                    )
                }?;
                (ptr, offset, bytes as u32, Some((slot.offset, slot.len)))
            }
        };
        unsafe {
            crate::global::compiled::materialize::init_compiled_program_image_from_summary(
                ptr.cast::<CompiledProgramFacts>(),
                summary,
            );
        }
        if let Some((old_offset, old_len)) = released_region {
            self.release_persistent_region(old_offset, old_len);
        }
        let slot = unsafe { &mut *self.program_images.add(insert_idx) };
        *slot = CompiledProgramSlot {
            stamp,
            offset,
            len: reserved_len,
            pins: 0,
            occupied: true,
        };
        Some(ptr.cast::<CompiledProgramFacts>())
    }

    #[inline]
    pub(crate) fn has_program_image(&self, stamp: ProgramStamp) -> bool {
        self.program_image_slot_index(stamp).is_some()
    }

    #[inline]
    pub(crate) fn program_image(&self, stamp: ProgramStamp) -> Option<*const CompiledProgramFacts> {
        let idx = self.program_image_slot_index(stamp)?;
        let slot = unsafe { &*self.program_images.add(idx) };
        Some(unsafe { self.program_image_from_slot(slot) })
    }

    #[inline(never)]
    unsafe fn pinned_role_image_from_slot(
        &mut self,
        slot: &RoleImageSlot,
    ) -> *const CompiledRoleImage {
        let (slab_ptr, _) = self.slab_ptr_and_len();
        let role_ptr = unsafe {
            slab_ptr
                .add(slot.offset as usize)
                .cast::<CompiledRoleImage>()
        };
        let reserved =
            Self::frontier_scratch_guard_bytes(unsafe { (*role_ptr).frontier_scratch_layout() })
                as u32;
        self.reserve_scratch_reserved_bytes(reserved);
        role_ptr
    }

    #[inline(never)]
    unsafe fn materialize_new_role_image_from_summary_for_program(
        &mut self,
        insert_idx: usize,
        stamp: ProgramStamp,
        role: u8,
        summary: &LoweringSummary,
        scratch: &mut RoleLoweringScratch<'_>,
        footprint: crate::global::role_program::RoleFootprint,
    ) -> Option<*const CompiledRoleImage> {
        let bytes = CompiledRoleImage::persistent_bytes_for_program(footprint);
        let (ptr, offset) = unsafe {
            self.allocate_persistent_image_bytes(bytes, CompiledRoleImage::persistent_align())
        }?;
        unsafe {
            crate::global::compiled::materialize::init_compiled_role_image_from_summary(
                ptr.cast::<CompiledRoleImage>(),
                role,
                summary,
                scratch,
                footprint,
            );
        }
        let actual_bytes = unsafe { (*ptr.cast::<CompiledRoleImage>()).actual_persistent_bytes() };
        if actual_bytes < bytes {
            self.release_persistent_region(
                offset.saturating_add(actual_bytes as u32),
                (bytes - actual_bytes) as u32,
            );
        }
        let reserved = Self::frontier_scratch_guard_bytes(unsafe {
            (*ptr.cast::<CompiledRoleImage>()).frontier_scratch_layout()
        }) as u32;
        self.reserve_scratch_reserved_bytes(reserved);
        let slot = unsafe { &mut *self.role_images.add(insert_idx) };
        debug_assert!(!slot.occupied);
        *slot = RoleImageSlot {
            stamp,
            role,
            offset,
            len: actual_bytes as u32,
            pins: 0,
            occupied: true,
        };
        Some(ptr.cast::<CompiledRoleImage>())
    }

    #[cfg(test)]
    #[inline(never)]
    pub(crate) unsafe fn materialize_role_image_from_summary<const ROLE: u8>(
        &mut self,
        stamp: ProgramStamp,
        summary: &LoweringSummary,
    ) -> Option<*const CompiledRoleImage> {
        let counts = summary.role_lowering_counts::<ROLE>();
        let footprint = crate::global::role_program::RoleFootprint {
            scope_count: counts.scope_count,
            eff_count: counts.eff_count,
            phase_count: counts.phase_count,
            phase_lane_entry_count: counts.phase_lane_entry_count,
            phase_lane_word_count: counts.phase_lane_word_count,
            parallel_enter_count: counts.parallel_enter_count,
            route_scope_count: counts.route_scope_count,
            local_step_count: counts.local_step_count,
            passive_linger_route_scope_count: counts.passive_linger_route_scope_count,
            active_lane_count: counts.active_lane_count,
            endpoint_lane_slot_count: counts.endpoint_lane_slot_count,
            logical_lane_count: counts.logical_lane_count,
            logical_lane_word_count: counts.logical_lane_word_count,
            max_route_stack_depth: 0,
            scope_evidence_count: 0,
            frontier_entry_count: 0,
        };
        let scratch_bytes =
            crate::global::compiled::materialize::role_lowering_scratch_storage_bytes(footprint);
        let mut scratch_storage = std::vec::Vec::with_capacity(scratch_bytes);
        scratch_storage.resize(scratch_bytes, 0u8);
        unsafe {
            crate::global::compiled::materialize::with_role_lowering_scratch_storage(
                footprint,
                scratch_storage.as_mut_ptr(),
                scratch_storage.len(),
                |scratch| {
                    self.materialize_role_image_from_summary_for_program::<ROLE>(
                        stamp, summary, scratch, footprint,
                    )
                },
            )
        }
        .flatten()
    }

    #[cfg(test)]
    #[inline(never)]
    pub(crate) unsafe fn materialize_role_image_from_summary_for_program<const ROLE: u8>(
        &mut self,
        stamp: ProgramStamp,
        summary: &LoweringSummary,
        scratch: &mut RoleLoweringScratch<'_>,
        footprint: crate::global::role_program::RoleFootprint,
    ) -> Option<*const CompiledRoleImage> {
        unsafe {
            self.materialize_role_image_from_summary_for_program_dyn(
                stamp, ROLE, summary, scratch, footprint,
            )
        }
    }

    #[inline(never)]
    pub(crate) unsafe fn materialize_role_image_from_summary_for_program_dyn(
        &mut self,
        stamp: ProgramStamp,
        role: u8,
        summary: &LoweringSummary,
        scratch: &mut RoleLoweringScratch<'_>,
        footprint: crate::global::role_program::RoleFootprint,
    ) -> Option<*const CompiledRoleImage> {
        if let Some(idx) = self.role_image_slot_index_for(stamp, role) {
            let slot = unsafe { &*self.role_images.add(idx) };
            return Some(unsafe { self.pinned_role_image_from_slot(slot) });
        }
        let Some(insert_idx) = self.first_free_role_image_slot() else {
            return unsafe {
                self.recycle_role_image_from_summary_for_program(
                    stamp, role, summary, scratch, footprint,
                )
            };
        };
        unsafe {
            self.materialize_new_role_image_from_summary_for_program(
                insert_idx, stamp, role, summary, scratch, footprint,
            )
        }
    }

    #[cold]
    #[inline(never)]
    unsafe fn recycle_role_image_from_summary_for_program(
        &mut self,
        stamp: ProgramStamp,
        role: u8,
        summary: &LoweringSummary,
        scratch: &mut RoleLoweringScratch<'_>,
        footprint: crate::global::role_program::RoleFootprint,
    ) -> Option<*const CompiledRoleImage> {
        let bytes = CompiledRoleImage::persistent_bytes_for_program(footprint);
        if let Some(insert_idx) = self.first_reusable_role_image_slot(bytes) {
            let slot = unsafe { &mut *self.role_images.add(insert_idx) };
            let ptr = unsafe {
                self.slab_ptr_and_len()
                    .0
                    .add(slot.offset as usize)
                    .cast::<CompiledRoleImage>()
            };
            unsafe {
                crate::global::compiled::materialize::init_compiled_role_image_from_summary(
                    ptr, role, summary, scratch, footprint,
                );
            }
            let actual_bytes = unsafe { (*ptr).actual_persistent_bytes() };
            if actual_bytes < slot.len as usize {
                self.release_persistent_region(
                    slot.offset.saturating_add(actual_bytes as u32),
                    slot.len.saturating_sub(actual_bytes as u32),
                );
            }
            let reserved =
                Self::frontier_scratch_guard_bytes(unsafe { (*ptr).frontier_scratch_layout() })
                    as u32;
            self.reserve_scratch_reserved_bytes(reserved);
            slot.stamp = stamp;
            slot.role = role;
            slot.len = actual_bytes as u32;
            slot.occupied = true;
            debug_assert_eq!(slot.pins, 0);
            return Some(ptr.cast_const());
        }
        let insert_idx = self.first_unpinned_role_image_slot()?;
        let (ptr, offset, reserved_len, released_region) = {
            let slot = unsafe { &*self.role_images.add(insert_idx) };
            if (slot.len as usize) >= bytes {
                let ptr = unsafe {
                    self.slab_ptr_and_len()
                        .0
                        .add(slot.offset as usize)
                        .cast::<CompiledRoleImage>()
                };
                (ptr.cast::<u8>(), slot.offset, slot.len, None)
            } else {
                let (ptr, offset) = unsafe {
                    self.allocate_persistent_image_bytes(
                        bytes,
                        CompiledRoleImage::persistent_align(),
                    )
                }?;
                (ptr, offset, bytes as u32, Some((slot.offset, slot.len)))
            }
        };
        unsafe {
            crate::global::compiled::materialize::init_compiled_role_image_from_summary(
                ptr.cast::<CompiledRoleImage>(),
                role,
                summary,
                scratch,
                footprint,
            );
        }
        let actual_bytes =
            unsafe { (*ptr.cast::<CompiledRoleImage>()).actual_persistent_bytes() } as u32;
        if actual_bytes < reserved_len {
            self.release_persistent_region(
                offset.saturating_add(actual_bytes),
                reserved_len.saturating_sub(actual_bytes),
            );
        }
        let reserved = Self::frontier_scratch_guard_bytes(unsafe {
            (*ptr.cast::<CompiledRoleImage>()).frontier_scratch_layout()
        }) as u32;
        self.reserve_scratch_reserved_bytes(reserved);
        if let Some((old_offset, old_len)) = released_region {
            self.release_persistent_region(old_offset, old_len);
        }
        let slot = unsafe { &mut *self.role_images.add(insert_idx) };
        *slot = RoleImageSlot {
            stamp,
            role,
            offset,
            len: actual_bytes,
            pins: 0,
            occupied: true,
        };
        Some(ptr.cast::<CompiledRoleImage>())
    }

    #[inline]
    pub(crate) fn has_role_image<const ROLE: u8>(&self, stamp: ProgramStamp) -> bool {
        self.role_image_slot_index_for(stamp, ROLE).is_some()
    }

    #[inline]
    pub(crate) fn role_image<const ROLE: u8>(
        &self,
        stamp: ProgramStamp,
    ) -> Option<*const CompiledRoleImage> {
        let (slab_ptr, _) = self.slab_ptr_and_len();
        let idx = self.role_image_slot_index_for(stamp, ROLE)?;
        let slot = unsafe { &*self.role_images.add(idx) };
        Some(unsafe {
            slab_ptr
                .add(slot.offset as usize)
                .cast::<CompiledRoleImage>()
        })
    }

    #[inline]
    pub(crate) unsafe fn allocate_endpoint_lease(
        &mut self,
        bytes: usize,
        align: usize,
        resident_budget: EndpointResidentBudget,
    ) -> Option<(EndpointLeaseId, u32, usize, usize)> {
        let (slab_ptr, slab_len) = self.slab_ptr_and_len();
        let base = slab_ptr as usize;
        let floor = self.endpoint_lease_floor();
        let mut candidate_end = slab_len;

        loop {
            let mut best_idx = None;
            let mut best_offset = 0usize;
            let mut idx = 0usize;
            while idx < usize::from(self.endpoint_lease_capacity) {
                let slot = unsafe { &*self.endpoint_leases.add(idx) };
                let offset = slot.offset as usize;
                if slot.occupied && offset < candidate_end && offset >= best_offset {
                    best_offset = offset;
                    best_idx = Some(idx);
                }
                idx += 1;
            }

            let gap_start = match best_idx {
                Some(idx) => {
                    let slot = unsafe { &*self.endpoint_leases.add(idx) };
                    slot.offset as usize + slot.len as usize
                }
                None => floor,
            };
            let gap_end = candidate_end;
            if gap_end >= bytes {
                let offset = Self::align_down(base + gap_end - bytes, align).saturating_sub(base);
                if offset >= gap_start && offset >= floor {
                    let mut insert_idx = 0usize;
                    while insert_idx < usize::from(self.endpoint_lease_capacity) {
                        let slot = unsafe { &mut *self.endpoint_leases.add(insert_idx) };
                        if !slot.occupied {
                            let generation = Self::next_endpoint_lease_generation(slot);
                            *slot = EndpointLeaseSlot {
                                generation,
                                offset: offset as u32,
                                len: bytes as u32,
                                resident_budget,
                                program_image_slot: u8::MAX,
                                role_image_slot: u8::MAX,
                                public_endpoint: false,
                                occupied: true,
                            };
                            let _ = slab_ptr;
                            return Some((
                                EndpointLeaseId::try_from(insert_idx).ok()?,
                                generation,
                                offset,
                                bytes,
                            ));
                        }
                        insert_idx += 1;
                    }
                    return None;
                }
            }

            let Some(idx) = best_idx else {
                break;
            };
            candidate_end = unsafe { (*self.endpoint_leases.add(idx)).offset as usize };
        }
        None
    }

    #[inline]
    pub(crate) fn release_endpoint_lease(&mut self, lease_slot: EndpointLeaseId, generation: u32) {
        let idx = usize::from(lease_slot);
        if idx >= usize::from(self.endpoint_lease_capacity) {
            return;
        }
        let slot = unsafe { &mut *self.endpoint_leases.add(idx) };
        if !slot.occupied || slot.generation != generation {
            return;
        }
        if slot.program_image_slot != u8::MAX {
            self.unpin_program_image_slot(slot.program_image_slot as usize);
        }
        if slot.role_image_slot != u8::MAX {
            self.unpin_role_image_slot(slot.role_image_slot as usize);
        }
        let generation = slot.generation;
        *slot = EndpointLeaseSlot {
            generation,
            ..EndpointLeaseSlot::EMPTY
        };
        self.trim_resident_headers_to_live_budget();
        self.recompute_scratch_reserved_bytes();
    }

    #[cfg(test)]
    #[inline]
    pub(crate) fn slot_bundle<'short>(&'short mut self) -> SlotBundle<'short>
    where
        'cfg: 'short,
    {
        let _ = self.ensure_slot_arena_storage();
        SlotBundle::new(&mut self.slot_arena)
    }

    pub(crate) fn register_policy(
        &mut self,
        lane: Lane,
        eff_index: EffIndex,
        tag: u8,
        policy: PolicyMode,
    ) -> Result<(), CpError> {
        if policy.is_dynamic() && self.ensure_policy_table_storage().is_none() {
            return Err(CpError::ResourceExhausted);
        }
        self.policies
            .register(lane, eff_index, tag, policy)
            .map_err(|_| CpError::ResourceExhausted)
    }

    pub(crate) fn policy(&self, lane: Lane, eff_index: EffIndex, tag: u8) -> Option<PolicyMode> {
        self.policies.get(lane, eff_index, tag)
    }

    pub(crate) fn reset_policy(&self, lane: Lane) {
        self.policies.reset_lane(lane);
    }

    #[inline]
    fn policy_digest(&self, slot: PolicySlot) -> u32 {
        let _ = slot;
        policy_runtime::POLICY_DIGEST_NONE
    }

    fn emit_effect(&self, effect: ControlOp, sid: SessionId, lane: Lane, arg: u32) {
        let event_id = control_op_tap_event_id(effect);
        let raw = lane.raw();
        debug_assert!(
            raw <= u32::from(u8::MAX),
            "lane id must fit within causal key encoding"
        );
        let causal = TapEvent::make_causal_key(raw as u8 + 1, 0);
        emit(
            self.tap(),
            RawEvent::new(self.clock.now32(), event_id)
                .with_causal_key(causal)
                .with_arg0(sid.raw())
                .with_arg1(arg),
        );
    }

    fn emit_topology_ack(
        &self,
        sid: SessionId,
        from_lane: Lane,
        to_lane: Lane,
        generation: Generation,
    ) {
        let packed = ((from_lane.as_wire() as u32) & 0xFF)
            | (((to_lane.as_wire() as u32) & 0xFF) << 8)
            | ((generation.0 as u32) << 16);
        emit(
            self.tap(),
            RawEvent::new(self.clock.now32(), crate::observe::ids::TOPOLOGY_ACK)
                .with_arg0(packed)
                .with_arg1(sid.raw()),
        );
    }

    fn emit_policy_event_with_arg2(
        &self,
        id: u16,
        lane: Option<Lane>,
        arg0: u32,
        arg1: u32,
        arg2: u32,
    ) {
        let causal = lane
            .map(|lane| {
                let raw = lane.raw();
                debug_assert!(
                    raw <= u32::from(u8::MAX),
                    "lane id must fit within causal key encoding"
                );
                let marker = raw as u8 + 1;
                TapEvent::make_causal_key(marker, 0)
            })
            .unwrap_or(0);

        emit(
            self.tap(),
            RawEvent::new(self.clock.now32(), id)
                .with_causal_key(causal)
                .with_arg0(arg0)
                .with_arg1(arg1)
                .with_arg2(arg2),
        );
    }

    fn perform_effect(&mut self, envelope: CpCommand) -> Result<(), CpError> {
        match envelope.effect {
            ControlOp::TopologyBegin => {
                let sid = envelope.sid.ok_or(CpError::Topology(
                    crate::control::cluster::error::TopologyError::InvalidSession,
                ))?;
                let sid = SessionId::new(sid.raw());
                let operands = envelope.topology.ok_or(CpError::Topology(
                    crate::control::cluster::error::TopologyError::InvalidState,
                ))?;
                self.topology_begin_from_intent(operands.intent(sid))
                    .map_err(map_topology_error)
            }
            ControlOp::TopologyAck => {
                let sid = envelope.sid.ok_or(CpError::Topology(
                    crate::control::cluster::error::TopologyError::InvalidSession,
                ))?;
                let operands = envelope.topology.ok_or(CpError::Topology(
                    crate::control::cluster::error::TopologyError::InvalidState,
                ))?;
                let intent = operands.intent(sid);
                let ack_expected = operands.ack(sid);

                let ack_result = self
                    .process_topology_intent(&intent)
                    .map_err(map_topology_error)?;

                if ack_result != ack_expected {
                    return Err(CpError::Topology(
                        crate::control::cluster::error::TopologyError::GenerationMismatch,
                    ));
                }

                self.emit_topology_ack(
                    SessionId::new(intent.sid),
                    intent.src_lane,
                    Lane::new(intent.dst_lane.raw()),
                    ack_result.new_gen,
                );
                Ok(())
            }
            ControlOp::TopologyCommit => {
                let sid = envelope.sid.ok_or(CpError::Topology(
                    crate::control::cluster::error::TopologyError::InvalidSession,
                ))?;
                let sid = SessionId::new(sid.raw());
                let operands = envelope.topology.ok_or(CpError::Topology(
                    crate::control::cluster::error::TopologyError::InvalidState,
                ))?;
                let lane = self
                    .validate_topology_commit_operands(sid, operands)
                    .map_err(map_topology_error)?;
                self.topology_commit(sid, lane)
                    .map_err(map_topology_error)?;
                Ok(())
            }
            ControlOp::CapDelegate => {
                let delegate = envelope.delegate.ok_or(CpError::Delegation(
                    crate::control::cluster::error::DelegationError::InvalidToken,
                ))?;

                let handle = delegate.token.endpoint_identity().map_err(|_| {
                    CpError::Delegation(
                        crate::control::cluster::error::DelegationError::InvalidToken,
                    )
                })?;
                let sid_raw = handle.sid.raw();
                let lane_raw = handle.lane.raw();

                if let Some(sid) = envelope.sid
                    && sid.raw() != sid_raw
                {
                    return Err(CpError::Delegation(
                        crate::control::cluster::error::DelegationError::InvalidToken,
                    ));
                }
                if let Some(lane) = envelope.lane
                    && lane.raw() != lane_raw
                {
                    return Err(CpError::Delegation(
                        crate::control::cluster::error::DelegationError::InvalidToken,
                    ));
                }

                let sid = SessionId::new(sid_raw);
                let lane = Lane::new(lane_raw);

                let ctx = EffectContext::new(sid, lane).with_delegate(DelegateContext {
                    claim: delegate.claim,
                    token: delegate.token,
                });

                match self.eval_effect(ControlOp::CapDelegate, ctx) {
                    Ok(_) => Ok(()),
                    Err(EffectError::Delegation(err)) => Err(map_delegate_error(err)),
                    Err(EffectError::Unsupported) => {
                        Err(CpError::UnsupportedEffect(ControlOp::CapDelegate as u8))
                    }
                    Err(EffectError::Topology(_))
                    | Err(EffectError::MissingGeneration)
                    | Err(EffectError::StateRestore(_))
                    | Err(EffectError::TxAbort(_))
                    | Err(EffectError::TxCommit(_)) => Err(CpError::Delegation(
                        crate::control::cluster::error::DelegationError::InvalidToken,
                    )),
                }
            }
            ControlOp::TxCommit => {
                let sid = envelope.sid.ok_or(CpError::TxCommit(
                    crate::control::cluster::error::TxCommitError::SessionNotFound,
                ))?;
                let lane = envelope.lane.ok_or(CpError::TxCommit(
                    crate::control::cluster::error::TxCommitError::SessionNotFound,
                ))?;
                let generation_input = envelope.generation.ok_or(CpError::TxCommit(
                    crate::control::cluster::error::TxCommitError::GenerationMismatch,
                ))?;
                let sid = SessionId::new(sid.raw());
                let lane = Lane::new(lane.raw());
                if self.assoc.get_sid(lane) != Some(sid) {
                    return Err(CpError::TxCommit(
                        crate::control::cluster::error::TxCommitError::SessionNotFound,
                    ));
                }
                self.tx_commit_at_lane(sid, lane, Generation(generation_input.raw()))
                    .map_err(map_tx_commit_error)
            }
            ControlOp::TxAbort => {
                let sid = envelope.sid.ok_or(CpError::TxAbort(
                    crate::control::cluster::error::TxAbortError::SessionNotFound,
                ))?;
                let lane = envelope.lane.ok_or(CpError::TxAbort(
                    crate::control::cluster::error::TxAbortError::SessionNotFound,
                ))?;
                let generation_input = envelope.generation.ok_or(CpError::TxAbort(
                    crate::control::cluster::error::TxAbortError::GenerationMismatch,
                ))?;
                let sid = SessionId::new(sid.raw());
                let lane = Lane::new(lane.raw());
                if self.assoc.get_sid(lane) != Some(sid) {
                    return Err(CpError::TxAbort(
                        crate::control::cluster::error::TxAbortError::SessionNotFound,
                    ));
                }
                self.tx_abort_at_lane(sid, lane, Generation(generation_input.raw()))
                    .map_err(map_tx_abort_error)
            }
            ControlOp::AbortBegin => {
                let sid = envelope.sid.ok_or(CpError::Abort(
                    crate::control::cluster::error::AbortError::SessionNotFound,
                ))?;
                let lane = envelope.lane.ok_or(CpError::Abort(
                    crate::control::cluster::error::AbortError::SessionNotFound,
                ))?;
                let sid = SessionId::new(sid.raw());
                let lane = Lane::new(lane.raw());
                if self.assoc.get_sid(lane) != Some(sid) {
                    return Err(CpError::Abort(
                        crate::control::cluster::error::AbortError::SessionNotFound,
                    ));
                }
                self.abort_begin_at_lane(sid, lane);
                Ok(())
            }
            ControlOp::AbortAck => {
                let sid = envelope.sid.ok_or(CpError::Abort(
                    crate::control::cluster::error::AbortError::SessionNotFound,
                ))?;
                let lane = envelope.lane.ok_or(CpError::Abort(
                    crate::control::cluster::error::AbortError::SessionNotFound,
                ))?;
                let generation_input = envelope.generation.ok_or(CpError::Abort(
                    crate::control::cluster::error::AbortError::GenerationMismatch,
                ))?;
                let sid = SessionId::new(sid.raw());
                let lane = Lane::new(lane.raw());
                if self.assoc.get_sid(lane) != Some(sid) {
                    return Err(CpError::Abort(
                        crate::control::cluster::error::AbortError::SessionNotFound,
                    ));
                }
                self.eval_effect(
                    ControlOp::AbortAck,
                    EffectContext::new(sid, lane)
                        .with_generation(Generation(generation_input.raw())),
                )
                .expect("abort ack evaluation must not fail");
                Ok(())
            }
            ControlOp::StateSnapshot => {
                let sid = envelope.sid.ok_or(CpError::StateSnapshot(
                    crate::control::cluster::error::StateSnapshotError::SessionNotFound,
                ))?;
                let lane = envelope.lane.ok_or(CpError::StateSnapshot(
                    crate::control::cluster::error::StateSnapshotError::SessionNotFound,
                ))?;
                let sid = SessionId::new(sid.raw());
                let lane = Lane::new(lane.raw());
                if self.assoc.get_sid(lane) != Some(sid) {
                    return Err(CpError::StateSnapshot(
                        crate::control::cluster::error::StateSnapshotError::SessionNotFound,
                    ));
                }
                let _ = self.state_snapshot_at_lane(sid, lane);
                Ok(())
            }
            ControlOp::StateRestore => {
                let sid = envelope.sid.ok_or(CpError::StateRestore(
                    crate::control::cluster::error::StateRestoreError::SessionNotFound,
                ))?;
                let lane = envelope.lane.ok_or(CpError::StateRestore(
                    crate::control::cluster::error::StateRestoreError::SessionNotFound,
                ))?;
                let generation_input = envelope.generation.ok_or(CpError::StateRestore(
                    crate::control::cluster::error::StateRestoreError::EpochMismatch,
                ))?;
                let sid = SessionId::new(sid.raw());
                let lane = Lane::new(lane.raw());
                if self.assoc.get_sid(lane) != Some(sid) {
                    return Err(CpError::StateRestore(
                        crate::control::cluster::error::StateRestoreError::SessionNotFound,
                    ));
                }
                self.state_restore_at_lane(sid, lane, Generation(generation_input.raw()))
                    .map_err(map_state_restore_error)
            }
            _ => Err(CpError::UnsupportedEffect(envelope.effect as u8)),
        }
    }

    fn eval_effect(
        &self,
        effect: ControlOp,
        ctx: EffectContext,
    ) -> Result<EffectResult, EffectError> {
        match effect {
            ControlOp::TopologyBegin => {
                self.ensure_authenticated_session_lane(ctx.sid, ctx.lane)
                    .map_err(EffectError::Topology)?;
                let target = ctx.generation.ok_or(EffectError::MissingGeneration)?;
                let mut prev = self.r#gen.last(ctx.lane);
                if prev.is_none() {
                    let _ = self.r#gen.check_and_update(ctx.lane, Generation(0));
                    prev = Some(Generation(0));
                }
                let prev = prev.unwrap_or(Generation(0));

                self.validate_topology_generation(ctx.lane, target)
                    .map_err(EffectError::Topology)?;

                let txn: Txn<LocalTopologyInvariant, IncreasingGen, One> =
                    unsafe { Txn::new(ctx.lane, prev) };
                let mut tap = NoopTap;
                let in_begin = txn.begin(&mut tap);
                let in_acked = in_begin.ack(&mut tap);

                let expected_ack = ctx.expected_topology_ack.ok_or(EffectError::Topology(
                    TopologyError::NoPending { lane: ctx.lane },
                ))?;
                let pending = PendingTopology::source_prepare(
                    ctx.sid,
                    ctx.lane,
                    Some(prev),
                    target,
                    in_acked,
                    ctx.fences,
                    expected_ack,
                );

                self.topology
                    .begin(ctx.lane, pending)
                    .map_err(EffectError::Topology)?;

                let packed = ((ctx.lane.as_wire() as u32) & 0xFF) | ((target.0 as u32) << 16);
                self.emit_effect(effect, ctx.sid, ctx.lane, packed);
                Ok(EffectResult::Generation(target))
            }
            ControlOp::TopologyAck => Ok(EffectResult::None),
            ControlOp::TopologyCommit => {
                let pending = self.topology.take(ctx.lane).ok_or(EffectError::Topology(
                    TopologyError::NoPending { lane: ctx.lane },
                ))?;

                let (
                    sid,
                    lane,
                    previous_generation,
                    target,
                    lease_state,
                    state,
                    fences,
                    expected_ack,
                ) = pending.into_parts();

                if sid != ctx.sid {
                    // Reinsert to preserve state before returning error.
                    let _ = self.topology.begin(
                        lane,
                        PendingTopology::source_prepare(
                            sid,
                            lane,
                            previous_generation,
                            target,
                            state.expect("topology commit reinsert requires a pending transaction"),
                            fences,
                            expected_ack
                                .expect("source topology reinsert requires an expected ack"),
                        ),
                    );
                    return Err(EffectError::Topology(TopologyError::UnknownSession {
                        sid: ctx.sid,
                    }));
                }

                self.validate_topology_generation(ctx.lane, target)
                    .map_err(EffectError::Topology)?;

                if let Err(err) = self.r#gen.check_and_update(ctx.lane, target) {
                    let _ = self.topology.begin(
                        lane,
                        PendingTopology::source_prepare(
                            sid,
                            lane,
                            previous_generation,
                            target,
                            state.expect("topology commit reinsert requires a pending transaction"),
                            fences,
                            expected_ack
                                .expect("source topology reinsert requires an expected ack"),
                        ),
                    );
                    let topology_err = match err {
                        GenError::StaleOrDuplicate(GenerationRecord { lane, last, new }) => {
                            TopologyError::StaleGeneration { lane, last, new }
                        }
                        GenError::Overflow { lane, last } => {
                            TopologyError::GenerationOverflow { lane, last }
                        }
                        GenError::InvalidInitial { lane, new } => {
                            TopologyError::InvalidInitial { lane, new }
                        }
                    };
                    return Err(EffectError::Topology(topology_err));
                }
                let _ = (lease_state, fences, expected_ack);

                let mut tap = NoopTap;
                let _closed = state
                    .expect("topology commit requires a pending transaction")
                    .commit(&mut tap);

                let packed = ((ctx.lane.as_wire() as u32) & 0xFF) | ((target.0 as u32) << 16);
                self.emit_effect(effect, ctx.sid, ctx.lane, packed);
                Ok(EffectResult::Generation(target))
            }
            ControlOp::CapDelegate => {
                let Some(delegate) = ctx.delegate else {
                    return Err(EffectError::Unsupported);
                };

                let token = delegate.token;
                let handle = token
                    .endpoint_identity()
                    .map_err(|_| EffectError::Delegation(super::error::CapError::Mismatch))?;
                let nonce = token.nonce();
                let sid_raw = handle.sid.raw();
                let lane_raw = handle.lane.raw();

                if sid_raw != ctx.sid.raw() || lane_raw != ctx.lane.raw() {
                    return Err(EffectError::Delegation(super::error::CapError::Mismatch));
                }

                if !delegate.claim {
                    self.mint_cap::<EndpointResource>(
                        ctx.sid,
                        ctx.lane,
                        CapShot::One,
                        handle.role,
                        nonce,
                        handle,
                    )
                    .map_err(EffectError::Delegation)?;
                    emit(
                        self.tap(),
                        DelegBegin::new(
                            self.clock.now32(),
                            ctx.sid.raw(),
                            ctx.lane.as_wire() as u32,
                        ),
                    );
                    Ok(EffectResult::None)
                } else {
                    self.claim_cap(&token)
                        .map(|_cap| EffectResult::None)
                        .map_err(EffectError::Delegation)
                }
            }
            ControlOp::TxCommit => {
                let generation = ctx.generation.ok_or(EffectError::MissingGeneration)?;
                let snapshot =
                    self.state_snapshots
                        .last_snapshot(ctx.lane)
                        .ok_or(EffectError::TxCommit(TxCommitError::NoStateSnapshot {
                            sid: ctx.sid,
                        }))?;

                if !matches!(
                    self.state_snapshots.finalization(ctx.lane),
                    None | Some(SnapshotFinalization::Available)
                ) {
                    return Err(EffectError::TxCommit(TxCommitError::AlreadyFinalized {
                        sid: ctx.sid,
                    }));
                }

                if snapshot != generation {
                    return Err(EffectError::TxCommit(TxCommitError::GenerationMismatch {
                        sid: ctx.sid,
                        expected: snapshot,
                        got: generation,
                    }));
                }

                self.state_snapshots.mark_committed(ctx.lane);
                self.caps.discard_released_lane_entries(ctx.lane);
                self.emit_effect(effect, ctx.sid, ctx.lane, generation.0 as u32);
                Ok(EffectResult::Generation(generation))
            }
            ControlOp::AbortBegin => {
                self.emit_effect(effect, ctx.sid, ctx.lane, ctx.lane.as_wire() as u32);
                Ok(EffectResult::None)
            }
            ControlOp::AbortAck => {
                let generation = ctx.generation.ok_or(EffectError::MissingGeneration)?;
                self.emit_effect(effect, ctx.sid, ctx.lane, generation.0 as u32);
                Ok(EffectResult::None)
            }
            ControlOp::StateSnapshot => {
                let epoch = self.r#gen.last(ctx.lane).unwrap_or(Generation(0));
                self.caps.discard_released_lane_entries(ctx.lane);
                self.state_snapshots
                    .record_snapshot(ctx.lane, epoch, self.cap_revision.get());
                self.emit_effect(effect, ctx.sid, ctx.lane, epoch.0 as u32);
                Ok(EffectResult::Generation(epoch))
            }
            ControlOp::StateRestore => {
                let requested = ctx.generation.ok_or(EffectError::MissingGeneration)?;
                let current = self.r#gen.last(ctx.lane).unwrap_or(Generation(0));
                let snapshot = self.state_snapshots.last_snapshot(ctx.lane).ok_or({
                    EffectError::StateRestore(StateRestoreError::NoStateSnapshot { sid: ctx.sid })
                })?;

                if !matches!(
                    self.state_snapshots.finalization(ctx.lane),
                    None | Some(SnapshotFinalization::Available)
                ) {
                    return Err(EffectError::StateRestore(
                        StateRestoreError::AlreadyFinalized { sid: ctx.sid },
                    ));
                }

                if requested != snapshot {
                    return Err(EffectError::StateRestore(
                        StateRestoreError::StaleStateSnapshot {
                            sid: ctx.sid,
                            requested,
                            current: snapshot,
                        },
                    ));
                }

                if current.raw() < requested.raw() {
                    return Err(EffectError::StateRestore(
                        StateRestoreError::EpochMismatch {
                            expected: current,
                            got: requested,
                        },
                    ));
                }

                let snapshot_cap_revision =
                    self.state_snapshots.last_cap_revision(ctx.lane).ok_or({
                        EffectError::StateRestore(StateRestoreError::NoStateSnapshot {
                            sid: ctx.sid,
                        })
                    })?;

                self.r#gen.restore_to(ctx.lane, requested).map_err(|_| {
                    EffectError::StateRestore(StateRestoreError::EpochMismatch {
                        expected: current,
                        got: requested,
                    })
                })?;
                self.restore_lane_runtime_state(ctx.lane, snapshot_cap_revision);
                self.state_snapshots.mark_restored(ctx.lane);

                self.emit_effect(effect, ctx.sid, ctx.lane, requested.0 as u32);
                emit(
                    self.tap(),
                    StateRestoreOk::new(self.clock.now32(), ctx.sid.raw(), requested.0 as u32),
                );

                Ok(EffectResult::Generation(requested))
            }
            ControlOp::TxAbort => {
                let requested = ctx.generation.ok_or(EffectError::MissingGeneration)?;
                let current = self.r#gen.last(ctx.lane).unwrap_or(Generation(0));
                let snapshot = self.state_snapshots.last_snapshot(ctx.lane).ok_or({
                    EffectError::TxAbort(TxAbortError::NoStateSnapshot { sid: ctx.sid })
                })?;

                if !matches!(
                    self.state_snapshots.finalization(ctx.lane),
                    None | Some(SnapshotFinalization::Available)
                ) {
                    return Err(EffectError::TxAbort(TxAbortError::AlreadyFinalized {
                        sid: ctx.sid,
                    }));
                }

                if requested != snapshot {
                    return Err(EffectError::TxAbort(TxAbortError::StaleStateSnapshot {
                        sid: ctx.sid,
                        requested,
                        current: snapshot,
                    }));
                }

                if current.raw() < requested.raw() {
                    return Err(EffectError::TxAbort(TxAbortError::GenerationMismatch {
                        sid: ctx.sid,
                        expected: current,
                        got: requested,
                    }));
                }

                let snapshot_cap_revision =
                    self.state_snapshots.last_cap_revision(ctx.lane).ok_or({
                        EffectError::TxAbort(TxAbortError::NoStateSnapshot { sid: ctx.sid })
                    })?;

                self.r#gen.restore_to(ctx.lane, requested).map_err(|_| {
                    EffectError::TxAbort(TxAbortError::GenerationMismatch {
                        sid: ctx.sid,
                        expected: current,
                        got: requested,
                    })
                })?;
                self.restore_lane_runtime_state(ctx.lane, snapshot_cap_revision);
                self.state_snapshots.mark_restored(ctx.lane);

                self.emit_effect(effect, ctx.sid, ctx.lane, requested.0 as u32);
                Ok(EffectResult::Generation(requested))
            }
            _ => Err(EffectError::Unsupported),
        }
    }
}

impl<'rv, 'cfg, T: Transport, U: LabelUniverse, C: Clock>
    Rendezvous<'rv, 'cfg, T, U, C, crate::control::cap::mint::EpochTbl>
where
    'cfg: 'rv,
{
    #[inline]
    const fn recommended_image_slot_capacity(endpoint_slots: usize) -> usize {
        if endpoint_slots == 0 {
            0
        } else {
            let doubled = endpoint_slots.saturating_mul(2);
            let widened = if doubled < 4 { 4 } else { doubled };
            if widened > u8::MAX as usize {
                u8::MAX as usize
            } else {
                widened
            }
        }
    }

    fn runtime_metadata_layout_with_image_slots(
        slab: &mut [u8],
        endpoint_slots: usize,
        image_slots: usize,
    ) -> Option<(usize, usize, usize, u32, usize, EndpointLeaseId)> {
        let base = slab.as_mut_ptr() as usize;
        let len = slab.len();

        let program_offset = Self::align_up(base, core::mem::align_of::<CompiledProgramSlot>());
        let program_bytes = image_slots.checked_mul(core::mem::size_of::<CompiledProgramSlot>())?;
        let role_offset = Self::align_up(
            program_offset.checked_add(program_bytes)?,
            core::mem::align_of::<RoleImageSlot>(),
        );
        let role_bytes = image_slots.checked_mul(core::mem::size_of::<RoleImageSlot>())?;
        let lease_offset = Self::align_up(
            role_offset.checked_add(role_bytes)?,
            core::mem::align_of::<EndpointLeaseSlot>(),
        );
        let lease_bytes = endpoint_slots.checked_mul(core::mem::size_of::<EndpointLeaseSlot>())?;
        let program_offset = program_offset.wrapping_sub(base);
        let role_offset = role_offset.wrapping_sub(base);
        let lease_offset = lease_offset.wrapping_sub(base);
        let frontier = lease_offset.checked_add(lease_bytes)?;
        if frontier > len {
            return None;
        }

        Some((
            program_offset,
            role_offset,
            lease_offset,
            frontier as u32,
            image_slots,
            EndpointLeaseId::try_from(endpoint_slots).ok()?,
        ))
    }

    unsafe fn init_runtime_metadata_with_image_slots(
        slab: &mut [u8],
        endpoint_slots: usize,
        image_slots: usize,
    ) -> Option<(
        *mut CompiledProgramSlot,
        *mut RoleImageSlot,
        *mut EndpointLeaseSlot,
        u32,
        u8,
        EndpointLeaseId,
    )> {
        let (
            program_offset,
            role_offset,
            lease_offset,
            frontier,
            image_slots,
            endpoint_lease_capacity,
        ) = Self::runtime_metadata_layout_with_image_slots(slab, endpoint_slots, image_slots)?;
        let base = slab.as_mut_ptr();
        let program_ptr = unsafe { base.add(program_offset).cast::<CompiledProgramSlot>() };
        let role_ptr = unsafe { base.add(role_offset).cast::<RoleImageSlot>() };
        let lease_ptr = unsafe { base.add(lease_offset).cast::<EndpointLeaseSlot>() };

        let mut idx = 0usize;
        while idx < image_slots {
            unsafe {
                program_ptr.add(idx).write(CompiledProgramSlot::EMPTY);
                role_ptr.add(idx).write(RoleImageSlot::EMPTY);
            }
            idx += 1;
        }

        idx = 0;
        while idx < endpoint_slots {
            unsafe {
                lease_ptr.add(idx).write(EndpointLeaseSlot::EMPTY);
            }
            idx += 1;
        }

        Some((
            program_ptr,
            role_ptr,
            lease_ptr,
            frontier,
            image_slots.min(u8::MAX as usize) as u8,
            endpoint_lease_capacity,
        ))
    }

    #[cfg(test)]
    unsafe fn init_runtime_metadata(
        slab: &mut [u8],
        endpoint_slots: usize,
    ) -> Option<(
        *mut CompiledProgramSlot,
        *mut RoleImageSlot,
        *mut EndpointLeaseSlot,
        u32,
        u8,
        EndpointLeaseId,
    )> {
        unsafe {
            Self::init_runtime_metadata_with_image_slots(slab, endpoint_slots, endpoint_slots)
        }
    }

    fn runtime_metadata_layout_for_public_path(
        slab: &mut [u8],
        endpoint_slots: usize,
    ) -> Option<(usize, usize, usize, u32, usize, EndpointLeaseId)> {
        let baseline =
            Self::runtime_metadata_layout_with_image_slots(slab, endpoint_slots, endpoint_slots)?;

        let desired = Self::recommended_image_slot_capacity(endpoint_slots);
        let mut image_slots = desired;
        loop {
            if let Some(layout) =
                Self::runtime_metadata_layout_with_image_slots(slab, endpoint_slots, image_slots)
            {
                return Some(layout);
            }
            if image_slots == endpoint_slots {
                return Some(baseline);
            }
            image_slots -= 1;
            if image_slots < endpoint_slots {
                image_slots = endpoint_slots;
            }
        }
    }

    #[cfg(test)]
    unsafe fn init_runtime_metadata_for_public_path(
        slab: &mut [u8],
        endpoint_slots: usize,
    ) -> Option<(
        *mut CompiledProgramSlot,
        *mut RoleImageSlot,
        *mut EndpointLeaseSlot,
        u32,
        u8,
        EndpointLeaseId,
    )> {
        let (_, _, _, _, image_slots, endpoint_lease_capacity) =
            Self::runtime_metadata_layout_for_public_path(slab, endpoint_slots)?;
        unsafe {
            Self::init_runtime_metadata_with_image_slots(
                slab,
                usize::from(endpoint_lease_capacity),
                image_slots,
            )
        }
    }

    unsafe fn init_runtime_metadata_for_public_path_auto(
        slab: &mut [u8],
        endpoint_slots: usize,
    ) -> Option<(
        *mut CompiledProgramSlot,
        *mut RoleImageSlot,
        *mut EndpointLeaseSlot,
        u32,
        u8,
        EndpointLeaseId,
    )> {
        let (_, _, _, _, image_slots, endpoint_lease_capacity) =
            Self::runtime_metadata_layout_for_public_path(slab, endpoint_slots)?;
        unsafe {
            Self::init_runtime_metadata_with_image_slots(
                slab,
                usize::from(endpoint_lease_capacity),
                image_slots,
            )
        }
    }

    unsafe fn carve_resident_storage(slab: &mut [u8]) -> Option<(*mut Self, &mut [u8])> {
        let base = slab.as_mut_ptr() as usize;
        let len = slab.len();
        let header_offset = Self::align_up(base, core::mem::align_of::<Self>());
        let header_end = header_offset.checked_add(core::mem::size_of::<Self>())?;
        let runtime_offset = header_end.wrapping_sub(base);
        if runtime_offset > len {
            return None;
        }
        let runtime_ptr = unsafe { slab.as_mut_ptr().add(runtime_offset) };
        let runtime_len = len - runtime_offset;
        let runtime_slab = unsafe { core::slice::from_raw_parts_mut(runtime_ptr, runtime_len) };
        Some((header_offset as *mut Self, runtime_slab))
    }

    #[cfg(test)]
    unsafe fn init_from_parts(
        dst: *mut Self,
        rv_id: RendezvousId,
        tap_buf: &'cfg mut [crate::observe::core::TapEvent; crate::runtime::consts::RING_EVENTS],
        slab: &mut [u8],
        lane_range: core::ops::Range<u8>,
        clock: C,
        liveness_policy: crate::runtime::config::LivenessPolicy,
        transport: T,
        endpoint_slots: usize,
    ) {
        let (
            program_images,
            role_images,
            endpoint_leases,
            image_frontier,
            image_slot_capacity,
            endpoint_lease_capacity,
        ) = unsafe {
            Self::init_runtime_metadata(slab, endpoint_slots).unwrap_or((
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                slab.len() as u32,
                0,
                EndpointLeaseId::ZERO,
            ))
        };

        unsafe {
            core::ptr::addr_of_mut!((*dst).brand_marker).write(PhantomData);
            core::ptr::addr_of_mut!((*dst).id).write(rv_id);
            core::ptr::addr_of_mut!((*dst).tap).write(TapRing::from_storage(tap_buf));
            core::ptr::addr_of_mut!((*dst).slab).write(slab as *mut [u8]);
            core::ptr::addr_of_mut!((*dst).slab_marker).write(PhantomData);
            core::ptr::addr_of_mut!((*dst).image_frontier).write(image_frontier);
            core::ptr::addr_of_mut!((*dst).scratch_reserved_bytes).write(0);
            core::ptr::addr_of_mut!((*dst).program_images).write(program_images);
            core::ptr::addr_of_mut!((*dst).role_images).write(role_images);
            core::ptr::addr_of_mut!((*dst).endpoint_leases).write(endpoint_leases);
            core::ptr::addr_of_mut!((*dst).image_slot_capacity).write(image_slot_capacity);
            core::ptr::addr_of_mut!((*dst).endpoint_lease_capacity).write(endpoint_lease_capacity);
            core::ptr::addr_of_mut!((*dst).runtime_frontier).write(image_frontier);
            core::ptr::addr_of_mut!((*dst).free_regions)
                .write([FreeRegion::EMPTY; FREE_REGION_CAPACITY]);
            core::ptr::addr_of_mut!((*dst).lane_range)
                .write((lane_range.start as u32)..(lane_range.end as u32));
            core::ptr::addr_of_mut!((*dst).universe_marker).write(PhantomData);
            core::ptr::addr_of_mut!((*dst).transport).write(transport);
            GenTable::init_empty(core::ptr::addr_of_mut!((*dst).r#gen));
            AssocTable::init_empty(core::ptr::addr_of_mut!((*dst).assoc));
            StateSnapshotTable::init_empty(core::ptr::addr_of_mut!((*dst).state_snapshots));
            TopologyStateTable::init_empty(core::ptr::addr_of_mut!((*dst).topology));
            core::ptr::addr_of_mut!((*dst).cap_nonce).write(Cell::new(0));
            core::ptr::addr_of_mut!((*dst).cap_revision).write(Cell::new(0));
            CapTable::init_empty(core::ptr::addr_of_mut!((*dst).caps));
            LoopTable::init_empty(core::ptr::addr_of_mut!((*dst).loops));
            RouteTable::init_empty(core::ptr::addr_of_mut!((*dst).routes));
            PolicyTable::init_empty(core::ptr::addr_of_mut!((*dst).policies));
            SlotArena::init_empty(core::ptr::addr_of_mut!((*dst).slot_arena));
            core::ptr::addr_of_mut!((*dst).clock).write(clock);
            core::ptr::addr_of_mut!((*dst).liveness_policy).write(liveness_policy);
            core::ptr::addr_of_mut!((*dst)._epoch_marker).write(PhantomData);
        }
    }

    /// Materialize a rendezvous directly into the borrowed slab prefix.
    ///
    /// # Safety
    /// The returned pointer is valid for as long as the borrowed slab storage lives.
    #[cfg(test)]
    pub(crate) unsafe fn init_in_slab(
        rv_id: RendezvousId,
        config: Config<'cfg, U, C>,
        transport: T,
        endpoint_slots: usize,
    ) -> Option<*mut Self> {
        let ConfigParts {
            tap_buf,
            slab,
            lane_range,
            clock,
            liveness_policy,
        } = config.into_parts();
        let (dst, runtime_slab) = unsafe { Self::carve_resident_storage(slab) }?;
        let (
            program_images,
            role_images,
            endpoint_leases,
            image_frontier,
            image_slot_capacity,
            endpoint_lease_capacity,
        ) = unsafe {
            Self::init_runtime_metadata_for_public_path(runtime_slab, endpoint_slots).unwrap_or((
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                runtime_slab.len() as u32,
                0,
                EndpointLeaseId::ZERO,
            ))
        };
        unsafe {
            core::ptr::addr_of_mut!((*dst).brand_marker).write(PhantomData);
            core::ptr::addr_of_mut!((*dst).id).write(rv_id);
            core::ptr::addr_of_mut!((*dst).tap).write(TapRing::from_storage(tap_buf));
            core::ptr::addr_of_mut!((*dst).slab).write(runtime_slab as *mut [u8]);
            core::ptr::addr_of_mut!((*dst).slab_marker).write(PhantomData);
            core::ptr::addr_of_mut!((*dst).image_frontier).write(image_frontier);
            core::ptr::addr_of_mut!((*dst).scratch_reserved_bytes).write(0);
            core::ptr::addr_of_mut!((*dst).program_images).write(program_images);
            core::ptr::addr_of_mut!((*dst).role_images).write(role_images);
            core::ptr::addr_of_mut!((*dst).endpoint_leases).write(endpoint_leases);
            core::ptr::addr_of_mut!((*dst).image_slot_capacity).write(image_slot_capacity);
            core::ptr::addr_of_mut!((*dst).endpoint_lease_capacity).write(endpoint_lease_capacity);
            core::ptr::addr_of_mut!((*dst).runtime_frontier).write(image_frontier);
            core::ptr::addr_of_mut!((*dst).free_regions)
                .write([FreeRegion::EMPTY; FREE_REGION_CAPACITY]);
            core::ptr::addr_of_mut!((*dst).lane_range)
                .write((lane_range.start as u32)..(lane_range.end as u32));
            core::ptr::addr_of_mut!((*dst).universe_marker).write(PhantomData);
            core::ptr::addr_of_mut!((*dst).transport).write(transport);
            GenTable::init_empty(core::ptr::addr_of_mut!((*dst).r#gen));
            AssocTable::init_empty(core::ptr::addr_of_mut!((*dst).assoc));
            StateSnapshotTable::init_empty(core::ptr::addr_of_mut!((*dst).state_snapshots));
            TopologyStateTable::init_empty(core::ptr::addr_of_mut!((*dst).topology));
            core::ptr::addr_of_mut!((*dst).cap_nonce).write(Cell::new(0));
            core::ptr::addr_of_mut!((*dst).cap_revision).write(Cell::new(0));
            CapTable::init_empty(core::ptr::addr_of_mut!((*dst).caps));
            LoopTable::init_empty(core::ptr::addr_of_mut!((*dst).loops));
            RouteTable::init_empty(core::ptr::addr_of_mut!((*dst).routes));
            PolicyTable::init_empty(core::ptr::addr_of_mut!((*dst).policies));
            SlotArena::init_empty(core::ptr::addr_of_mut!((*dst).slot_arena));
            core::ptr::addr_of_mut!((*dst).clock).write(clock);
            core::ptr::addr_of_mut!((*dst).liveness_policy).write(liveness_policy);
            core::ptr::addr_of_mut!((*dst)._epoch_marker).write(PhantomData);
        }
        unsafe {
            if (&mut *dst).ensure_core_lane_storage().is_none() {
                Self::cleanup_failed_public_init(dst);
                return None;
            }
        }
        Some(dst)
    }

    /// Materialize a rendezvous directly into the borrowed slab prefix using a
    /// public-path endpoint capacity derived from the runtime slab owner.
    ///
    /// # Safety
    /// The returned pointer is valid for as long as the borrowed slab storage lives.
    pub(crate) unsafe fn init_in_slab_auto(
        rv_id: RendezvousId,
        config: Config<'cfg, U, C>,
        transport: T,
        endpoint_slots: usize,
    ) -> Option<*mut Self> {
        let ConfigParts {
            tap_buf,
            slab,
            lane_range,
            clock,
            liveness_policy,
        } = config.into_parts();
        let (dst, runtime_slab) = unsafe { Self::carve_resident_storage(slab) }?;
        let (
            program_images,
            role_images,
            endpoint_leases,
            image_frontier,
            image_slot_capacity,
            endpoint_lease_capacity,
        ) = unsafe {
            Self::init_runtime_metadata_for_public_path_auto(runtime_slab, endpoint_slots)
                .unwrap_or((
                    core::ptr::null_mut(),
                    core::ptr::null_mut(),
                    core::ptr::null_mut(),
                    runtime_slab.len() as u32,
                    0,
                    EndpointLeaseId::ZERO,
                ))
        };
        unsafe {
            core::ptr::addr_of_mut!((*dst).brand_marker).write(PhantomData);
            core::ptr::addr_of_mut!((*dst).id).write(rv_id);
            core::ptr::addr_of_mut!((*dst).tap).write(TapRing::from_storage(tap_buf));
            core::ptr::addr_of_mut!((*dst).slab).write(runtime_slab as *mut [u8]);
            core::ptr::addr_of_mut!((*dst).slab_marker).write(PhantomData);
            core::ptr::addr_of_mut!((*dst).image_frontier).write(image_frontier);
            core::ptr::addr_of_mut!((*dst).scratch_reserved_bytes).write(0);
            core::ptr::addr_of_mut!((*dst).program_images).write(program_images);
            core::ptr::addr_of_mut!((*dst).role_images).write(role_images);
            core::ptr::addr_of_mut!((*dst).endpoint_leases).write(endpoint_leases);
            core::ptr::addr_of_mut!((*dst).image_slot_capacity).write(image_slot_capacity);
            core::ptr::addr_of_mut!((*dst).endpoint_lease_capacity).write(endpoint_lease_capacity);
            core::ptr::addr_of_mut!((*dst).runtime_frontier).write(image_frontier);
            core::ptr::addr_of_mut!((*dst).free_regions)
                .write([FreeRegion::EMPTY; FREE_REGION_CAPACITY]);
            core::ptr::addr_of_mut!((*dst).lane_range)
                .write((lane_range.start as u32)..(lane_range.end as u32));
            core::ptr::addr_of_mut!((*dst).universe_marker).write(PhantomData);
            core::ptr::addr_of_mut!((*dst).transport).write(transport);
            GenTable::init_empty(core::ptr::addr_of_mut!((*dst).r#gen));
            AssocTable::init_empty(core::ptr::addr_of_mut!((*dst).assoc));
            StateSnapshotTable::init_empty(core::ptr::addr_of_mut!((*dst).state_snapshots));
            TopologyStateTable::init_empty(core::ptr::addr_of_mut!((*dst).topology));
            core::ptr::addr_of_mut!((*dst).cap_nonce).write(Cell::new(0));
            core::ptr::addr_of_mut!((*dst).cap_revision).write(Cell::new(0));
            CapTable::init_empty(core::ptr::addr_of_mut!((*dst).caps));
            LoopTable::init_empty(core::ptr::addr_of_mut!((*dst).loops));
            RouteTable::init_empty(core::ptr::addr_of_mut!((*dst).routes));
            PolicyTable::init_empty(core::ptr::addr_of_mut!((*dst).policies));
            SlotArena::init_empty(core::ptr::addr_of_mut!((*dst).slot_arena));
            core::ptr::addr_of_mut!((*dst).clock).write(clock);
            core::ptr::addr_of_mut!((*dst).liveness_policy).write(liveness_policy);
            core::ptr::addr_of_mut!((*dst)._epoch_marker).write(PhantomData);
        }
        unsafe {
            if (&mut *dst).ensure_core_lane_storage().is_none() {
                Self::cleanup_failed_public_init(dst);
                return None;
            }
        }
        Some(dst)
    }

    /// Write a rendezvous directly into the destination slot.
    ///
    /// # Safety
    /// `dst` must point to valid, writable storage for `Self`.
    #[cfg(test)]
    pub(crate) unsafe fn init_from_config(
        dst: *mut Self,
        rv_id: RendezvousId,
        config: Config<'cfg, U, C>,
        transport: T,
        endpoint_slots: usize,
    ) {
        let ConfigParts {
            tap_buf,
            slab,
            lane_range,
            clock,
            liveness_policy,
        } = config.into_parts();
        unsafe {
            Self::init_from_parts(
                dst,
                rv_id,
                tap_buf,
                slab,
                lane_range,
                clock,
                liveness_policy,
                transport,
                endpoint_slots,
            );
            if (&mut *dst).ensure_core_lane_storage().is_none() {
                Self::cleanup_failed_public_init(dst);
                panic!("rendezvous test init must allocate lane-scoped storage");
            }
        }
    }
}

impl<'rv, 'cfg, T: Transport, U: LabelUniverse, C: Clock, E: crate::control::cap::mint::EpochTable>
    Rendezvous<'rv, 'cfg, T, U, C, E>
where
    'cfg: 'rv,
{
    pub(crate) fn initialise_control_scope(&self, lane: Lane, scope_kind: ControlScopeKind) {
        match scope_kind {
            ControlScopeKind::Loop => {
                self.loops.reset_lane(lane);
            }
            ControlScopeKind::State => {
                self.state_snapshots.reset_lane(lane);
            }
            ControlScopeKind::Abort => {}
            ControlScopeKind::Topology => {
                self.topology.reset_lane(lane);
            }
            ControlScopeKind::Delegate
            | ControlScopeKind::Policy
            | ControlScopeKind::Route
            | ControlScopeKind::None => {}
        }
    }

    #[inline]
    pub(crate) fn state_snapshot_at_lane(&self, sid: SessionId, lane: Lane) -> Generation {
        match self.eval_effect(ControlOp::StateSnapshot, EffectContext::new(sid, lane)) {
            Ok(EffectResult::Generation(epoch)) => epoch,
            Ok(EffectResult::None) => unreachable!("state snapshot effect must yield generation"),
            Err(_) => unreachable!("state snapshot effect cannot fail"),
        }
    }

    #[inline]
    pub(crate) fn tx_commit_at_lane(
        &self,
        sid: SessionId,
        lane: Lane,
        generation: Generation,
    ) -> Result<(), TxCommitError> {
        match self.eval_effect(
            ControlOp::TxCommit,
            EffectContext::new(sid, lane).with_generation(generation),
        ) {
            Ok(_) => Ok(()),
            Err(EffectError::TxCommit(err)) => Err(err),
            Err(EffectError::MissingGeneration)
            | Err(EffectError::Unsupported)
            | Err(EffectError::Topology(_))
            | Err(EffectError::Delegation(_))
            | Err(EffectError::TxAbort(_))
            | Err(EffectError::StateRestore(_)) => {
                unreachable!("tx commit effect failure is fully covered")
            }
        }
    }

    #[inline]
    pub(crate) fn tx_abort_at_lane(
        &self,
        sid: SessionId,
        lane: Lane,
        generation: Generation,
    ) -> Result<(), TxAbortError> {
        match self.eval_effect(
            ControlOp::TxAbort,
            EffectContext::new(sid, lane).with_generation(generation),
        ) {
            Ok(_) => Ok(()),
            Err(EffectError::TxAbort(err)) => Err(err),
            Err(EffectError::MissingGeneration)
            | Err(EffectError::Unsupported)
            | Err(EffectError::Topology(_))
            | Err(EffectError::Delegation(_))
            | Err(EffectError::TxCommit(_))
            | Err(EffectError::StateRestore(_)) => {
                unreachable!("tx abort effect failure is fully covered")
            }
        }
    }

    #[inline]
    pub(crate) fn abort_begin_at_lane(&self, sid: SessionId, lane: Lane) {
        self.eval_effect(ControlOp::AbortBegin, EffectContext::new(sid, lane))
            .expect("abort begin evaluation must not fail");
    }

    #[cfg(test)]
    pub(crate) fn is_session_registered(&self, sid: SessionId) -> bool {
        self.assoc.find_lane(sid).is_some()
    }

    #[inline]
    fn ensure_authenticated_session_lane(
        &self,
        sid: SessionId,
        lane: Lane,
    ) -> Result<(), TopologyError> {
        if self.assoc.get_sid(lane) == Some(sid) {
            Ok(())
        } else {
            Err(TopologyError::UnknownSession { sid })
        }
    }

    #[cfg(test)]
    pub(crate) fn session_lane(&self, sid: SessionId) -> Option<Lane> {
        self.assoc.find_lane(sid)
    }

    pub(crate) fn lane_generation(&self, lane: Lane) -> Generation {
        self.r#gen.last(lane).unwrap_or(Generation::ZERO)
    }

    pub(crate) fn snapshot_generation(&self, lane: Lane) -> Option<Generation> {
        self.state_snapshots.last_snapshot(lane)
    }

    pub(crate) fn expected_topology_ack(
        &self,
        sid: SessionId,
    ) -> Result<TopologyAck, TopologyError> {
        self.topology.expected_ack_for_session(sid)
    }

    pub(crate) fn topology_session_state(&self, sid: SessionId) -> Option<TopologySessionState> {
        self.topology.session_state(sid)
    }

    #[cfg(test)]
    pub(crate) fn advance_lane_generation_for_test(&self, lane: Lane, target: Generation) {
        if self.r#gen.last(lane).is_none() {
            let _ = self.r#gen.check_and_update(lane, Generation::ZERO);
        }
        if target != Generation::ZERO {
            self.r#gen
                .check_and_update(lane, target)
                .expect("test fixture lane generation must advance monotonically");
        }
    }

    pub(crate) fn release_lane(&self, lane: Lane) -> Option<SessionId> {
        let sid = self.assoc.get_sid(lane)?;
        let remaining = self.assoc.decrement(lane, sid)?;
        if remaining > 0 {
            return None;
        }
        self.reset_lane_state(lane);
        Some(sid)
    }

    fn reset_lane_state(&self, lane: Lane) {
        self.r#gen.reset_lane(lane);
        self.state_snapshots.reset_lane(lane);
        self.reset_lane_recycled_state(lane);
    }

    fn restore_lane_runtime_state(&self, lane: Lane, snapshot_cap_revision: u64) {
        self.topology.reset_lane(lane);
        self.caps
            .restore_lane_to_revision(lane, snapshot_cap_revision);
        self.loops.reset_lane(lane);
        self.routes.reset_lane(lane);
    }

    fn reset_lane_recycled_state(&self, lane: Lane) {
        self.topology.reset_lane(lane);
        self.caps.purge_lane(lane);
        self.loops.reset_lane(lane);
        self.routes.reset_lane(lane);
        self.policies.reset_lane(lane);
    }

    #[inline]
    pub(crate) fn emit_lane_release(&self, sid: SessionId, lane: Lane) {
        emit(
            self.tap(),
            LaneRelease::new(
                self.now32(),
                self.id.raw() as u32,
                sid.raw(),
                lane.raw() as u16,
            ),
        );
    }
}

/// **RAII witness for exclusive lane access.**
///
/// `LaneLease<'a, 'cfg, ...>` is the **affine witness** that guarantees exclusive access
/// to a transport lane. It is parameterized by a **borrow lifetime** `'a` to enforce
/// the invariant that **all leases must be dropped before the borrow expires**:
///
/// ```text
/// Drop order guarantee (enforced by lifetime 'a):
///   LaneLease<'a, ...> → Port<'a, ...> → &'a Rendezvous (borrow expires)
/// ```
///
/// The key insight is that `'a` is the **lifetime of the borrow** from `lease_port(&'a self)`,
/// which is **independent** of the `Rendezvous<'rv, 'cfg, ...>` invariant lifetime `'rv`.
/// This allows **nested scopes** where leases are dropped before the Rendezvous itself:
///
/// ```text
/// let mut rv = /* some Rendezvous owner */; // 'rv starts
/// {
///     let lease = rv.lease_port(...);     // 'a: shorter borrow
/// }                                        // 'a ends, lease dropped
///                                          // rv can now be moved/dropped
/// ```
///
/// # Type-Level Guarantees
///
/// 1. **Affine Linearity**: Each `LaneLease` owns a unique lane slot; moving or dropping
///    it revokes access to that lane.
/// 2. **Lifetime Binding**: The `'a` lifetime ensures that the lease does not outlive
///    the borrow of the `Rendezvous`.
/// 3. **RAII Release**: On drop, the lane is automatically released back to the
///    `Rendezvous` unless explicitly transferred via `into_port()`.
///
/// # Example
///
/// ```ignore
/// let mut rv = /* some Rendezvous owner */;
/// {
///     let lease = rv.lease_port(sid, lane, role)?;
///     let port = lease.port();
///     // ... use port
/// } // ← lease dropped here, lane released, borrow 'a expires
/// // ← rv can now be safely dropped or moved
/// ```
///
/// # POPL Justification
///
/// This design implements **separation logic** with **region polymorphism**:
/// - `LaneLease<'a, ...>` is the **ownership token** for a lane, valid during region `'a`.
/// - The borrow `'a` acts as the **region annotation** ensuring temporal safety.
/// - Drop implementation is the **linear consumption** that releases the resource.
/// - The distinction between `'rv` (invariant lifetime of Rendezvous) and `'a` (covariant
///   borrow lifetime) enables **flexible scoping** without sacrificing safety.
///
/// Affine MPST + RAII underpin the theoretical foundation for this module.
///
/// # Visibility
///
/// This type is internal implementation, hidden from public docs but
/// accessible to integration tests. Public API users obtain endpoints via
/// [`SessionKit::enter`](crate::substrate::SessionKit::enter).
///
/// # Cluster Ownership Model
///
/// `LaneLease` now owns the rendezvous lease outright. This ties the borrow
/// lifetime `'lease` to the rendezvous itself and removes the need for raw
/// pointers or `PhantomData` hacks. The ownership chain is purely typed:
/// Cluster → RendezvousLease → LaneLease.
///
/// # Safety Invariants (documented for POPL/SOSP/OSDI)
///
/// 1. `cluster_ptr` always points to a valid `SessionKit` during `'lease`
/// 2. Only `LaneLease::Drop` calls back into the cluster to release the lane
/// 3. SessionKit guarantees: no duplicate leases for same lane
/// 4. SessionKit guarantees: no Rendezvous write access while lease held
/// 5. Cluster must not move while lease is alive (enforced by the PhantomData borrow)
///
/// # Observable Properties
///
/// - LANE_ACQUIRE tap event on lease creation (via `SessionKit::lease_port`)
/// - LANE_RELEASE tap event on Drop
/// - Streaming checker verifies acquire/release pairs match (similar to cancel begin/ack)
pub(crate) struct LaneLease<'lease, 'cfg, T, U, C, const MAX_RV: usize>
where
    T: Transport,
    U: LabelUniverse + 'cfg,
    C: Clock + 'cfg,
    'cfg: 'lease,
{
    /// Borrow-bound lease over the parent rendezvous.
    lease: Option<
        crate::control::lease::core::RendezvousLease<
            'lease,
            'cfg,
            T,
            U,
            C,
            crate::control::cap::mint::EpochTbl,
            crate::control::lease::core::FullSpec,
        >,
    >,
    /// Session identifier.
    sid: SessionId,
    /// Lane identifier.
    lane: Lane,
    /// Role for the port.
    role: u8,
    /// Number of global roles participating in the attached program.
    role_count: u8,
    /// Active lease counter borrowed from the parent cluster.
    active_leases: Option<&'lease core::cell::Cell<u32>>,
    /// Rendezvous brand for typed owner construction.
    brand: crate::control::brand::Guard<'cfg>,
}

impl<'lease, 'cfg, T, U, C, const MAX_RV: usize> LaneLease<'lease, 'cfg, T, U, C, MAX_RV>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    'cfg: 'lease,
{
    /// Internal constructor (called by `SessionKit::lease_port`).
    /// The caller must ensure no duplicate leases for the same `(rv_id, lane)` pair.
    pub(crate) fn new(
        lease: crate::control::lease::core::RendezvousLease<
            'lease,
            'cfg,
            T,
            U,
            C,
            crate::control::cap::mint::EpochTbl,
            crate::control::lease::core::FullSpec,
        >,
        sid: SessionId,
        lane: Lane,
        role: u8,
        role_count: u8,
        active_leases: &'lease core::cell::Cell<u32>,
        brand: crate::control::brand::Guard<'cfg>,
    ) -> Self {
        Self {
            lease: Some(lease),
            sid,
            lane,
            role,
            role_count,
            active_leases: Some(active_leases),
            brand,
        }
    }

    #[allow(clippy::type_complexity)]
    pub(crate) fn into_port_guard(
        mut self,
    ) -> Result<
        (
            Port<'lease, T, crate::control::cap::mint::EpochTbl>,
            LaneGuard<'lease, T, U, C>,
            crate::control::brand::Guard<'cfg>,
        ),
        RendezvousError,
    > {
        let (port, guard) = {
            let lease = self
                .lease
                .as_mut()
                .expect("lane lease retains rendezvous lease");
            // SAFETY: `RendezvousLease<'lease, 'cfg, ...>` holds the unique mutable
            // entry borrow for `'lease`, so reborrowing the rendezvous as shared for
            // the same `'lease` lifetime is sound as long as we do not use the lease
            // mutably while the shared reference is live.
            let rv_ptr: *mut Rendezvous<'cfg, 'cfg, T, U, C, crate::control::cap::mint::EpochTbl> =
                lease.with_rendezvous(core::ptr::from_mut);
            let rv: &'lease Rendezvous<'cfg, 'cfg, T, U, C, crate::control::cap::mint::EpochTbl> =
                unsafe { &*rv_ptr };
            let active_leases = *self
                .active_leases
                .as_ref()
                .expect("lane lease retains active lease counter");
            rv.materialize_port_guard(
                self.sid,
                self.lane,
                self.role,
                self.role_count,
                active_leases,
            )?
        };
        drop(self.lease.take());
        let _ = self.active_leases.take();
        Ok((port, guard, self.brand))
    }

    #[inline]
    pub(crate) fn with_rendezvous_mut<R>(
        &mut self,
        f: impl FnOnce(&mut Rendezvous<'cfg, 'cfg, T, U, C, crate::control::cap::mint::EpochTbl>) -> R,
    ) -> R {
        let lease = self
            .lease
            .as_mut()
            .expect("lane lease retains rendezvous lease");
        lease.with_rendezvous(f)
    }
}

impl<'lease, 'cfg, T, U, C, const MAX_RV: usize> Drop for LaneLease<'lease, 'cfg, T, U, C, MAX_RV>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    'cfg: 'lease,
{
    fn drop(&mut self) {
        if let Some(mut lease) = self.lease.take() {
            lease.release_lane_with_tap(self.lane);
        }
        if let Some(active_leases) = self.active_leases.take() {
            let current = active_leases.get();
            debug_assert!(current > 0, "lane_release underflow");
            active_leases.set(current.saturating_sub(1));
        }
    }
}

impl<'rv, 'cfg, T: Transport, U: LabelUniverse, C: Clock, E: crate::control::cap::mint::EpochTable>
    Rendezvous<'rv, 'cfg, T, U, C, E>
where
    'cfg: 'rv,
{
    #[inline]
    pub(crate) fn brand(&self) -> Guard<'rv> {
        Guard::new()
    }

    /// Observability ring; pushing events only needs `&self` because the ring
    /// is single-producer and internally synchronised.
    pub(crate) fn tap(&self) -> &TapRing<'cfg> {
        &self.tap
    }

    #[inline]
    pub(crate) fn liveness_policy(&self) -> crate::runtime::config::LivenessPolicy {
        self.liveness_policy
    }

    pub(crate) fn now32(&self) -> u32 {
        self.clock.now32()
    }

    /// Access the capability table for token registration.
    #[inline]
    pub(crate) fn caps(&self) -> &CapTable {
        &self.caps
    }

    pub(crate) fn activate_lane_attachment(
        &self,
        sid: SessionId,
        lane: Lane,
    ) -> Result<(), RendezvousError> {
        if !self.lane_range.contains(&lane.0) {
            return Err(RendezvousError::LaneOutOfRange { lane });
        }
        let attach_ready_sid = self.topology.attach_ready_sid(lane);
        let first_attach = match self.assoc.get_sid(lane) {
            None => {
                if let Some(reserved_sid) = attach_ready_sid
                    && reserved_sid != sid
                {
                    return Err(RendezvousError::LaneBusy { lane });
                }
                self.assoc.register(lane, sid);
                true
            }
            Some(existing) if existing == sid => {
                if attach_ready_sid.is_some() {
                    return Err(RendezvousError::LaneBusy { lane });
                }
                self.assoc
                    .increment(lane, sid)
                    .expect("lane attachment count overflow");
                false
            }
            Some(_) => {
                return Err(RendezvousError::LaneBusy { lane });
            }
        };

        if first_attach {
            // Emit lane_open_tap_event_id() for the lane's inaugural attachment.
            emit(
                self.tap(),
                RawEvent::new(
                    self.clock.now32(),
                    crate::control::cluster::effects::lane_open_tap_event_id(),
                )
                .with_arg0(sid.raw())
                .with_arg1(lane.0),
            );

            if attach_ready_sid == Some(sid) {
                self.topology.reset_lane(lane);
                self.state_snapshots.reset_lane(lane);
                self.reset_lane_recycled_state(lane);
            } else {
                self.r#gen.reset_lane(lane);
                self.state_snapshots.reset_lane(lane);
                self.reset_lane_recycled_state(lane);
            }
        }
        Ok(())
    }

    #[allow(clippy::type_complexity)]
    fn materialize_port_guard<'a>(
        &'a self,
        sid: SessionId,
        lane: Lane,
        role: u8,
        role_count: u8,
        active_leases: &'a Cell<u32>,
    ) -> Result<
        (
            Port<'a, T, crate::control::cap::mint::EpochTbl>,
            LaneGuard<'a, T, U, C>,
        ),
        RendezvousError,
    >
    where
        'rv: 'a,
    {
        let (tx, rx) = self.transport.open(role, sid.raw());
        let port = Port::new(
            &self.transport,
            self.tap(),
            &self.clock,
            &self.loops,
            &self.routes,
            self.slab,
            core::ptr::addr_of!(self.image_frontier),
            core::ptr::addr_of!(self.scratch_reserved_bytes),
            self.endpoint_leases.cast_const(),
            self.endpoint_lease_capacity,
            lane,
            role,
            role_count,
            self.id,
            tx,
            rx,
        );
        let guard =
            LaneGuard::new_detached((self as *const Self).cast::<()>(), lane, active_leases);
        Ok((port, guard))
    }

    #[cfg(test)]
    pub(crate) fn activate_lane_for_test(
        &self,
        sid: SessionId,
        lane: Lane,
    ) -> Result<(), RendezvousError> {
        self.activate_lane_attachment(sid, lane)
    }

    // ============================================================================
    // Capability methods
    // ============================================================================

    #[inline]
    pub(crate) fn next_nonce_seed(&self) -> NonceSeed {
        let ordinal = self.cap_nonce.get();
        let next = ordinal
            .checked_add(1)
            .expect("capability nonce counter exhausted");
        self.cap_nonce.set(next);
        NonceSeed::counter(ordinal)
    }

    #[inline]
    pub(crate) fn next_cap_revision(&self) -> u64 {
        let next = self
            .cap_revision
            .get()
            .checked_add(1)
            .expect("capability revision counter exhausted");
        self.cap_revision.set(next);
        next
    }

    #[inline]
    pub(crate) fn cap_release_ctx(&self, lane: Lane) -> CapReleaseCtx {
        CapReleaseCtx::new(&self.caps, &self.state_snapshots, &self.cap_revision, lane)
    }

    pub(crate) fn mint_cap<K: ResourceKind>(
        &self,
        sid: SessionId,
        lane: Lane,
        shot: CapShot,
        dest_role: u8,
        nonce: [u8; 16],
        mut handle: K::Handle,
    ) -> Result<(), CapError> {
        let kind_tag = K::TAG;
        let registered_sid = self
            .assoc
            .get_sid(lane)
            .ok_or(CapError::WrongSessionOrLane)?;
        if registered_sid != sid {
            return Err(CapError::WrongSessionOrLane);
        }

        let handle_bytes = K::encode_handle(&handle);
        K::zeroize(&mut handle);

        let entry = CapEntry {
            sid,
            lane_raw: lane.as_wire(),
            kind_tag,
            shot_state: shot.as_u8(),
            role: dest_role,
            mint_revision: self.next_cap_revision(),
            consumed_revision: 0,
            released_revision: 0,
            nonce,
            handle: handle_bytes,
        };
        self.caps
            .insert_entry(entry)
            .map_err(|_| CapError::TableFull)?;

        let tap = self.tap();
        emit(
            tap,
            RawEvent::new(self.clock.now32(), crate::observe::cap_mint::<K>())
                .with_arg0(sid.raw())
                .with_arg1(((lane.as_wire() as u32) << 16) | (dest_role as u32)),
        );
        Ok(())
    }

    pub(crate) fn claim_cap<K: crate::control::cap::mint::ResourceKind>(
        &self,
        token: &GenericCapToken<K>,
    ) -> Result<VerifiedCap<K>, CapError> {
        let nonce = token.nonce();

        // Check if AUTO (all zeros)
        if nonce == [0u8; crate::control::cap::mint::CAP_NONCE_LEN] && token.is_auto() {
            return Err(CapError::UnknownToken);
        }

        let header = token.control_header().map_err(|_| CapError::Mismatch)?;
        if header.tag() == crate::control::cap::mint::EndpointResource::TAG {
            let endpoint_token = crate::control::cap::mint::GenericCapToken::<
                crate::control::cap::mint::EndpointResource,
            >::from_bytes(token.bytes);
            endpoint_token
                .endpoint_identity()
                .map_err(|_| CapError::Mismatch)?;
        }

        let sid = header.sid();
        let lane = header.lane();
        let role = header.role();
        let kind_tag = header.tag();
        let shot = match header.shot() {
            crate::control::cap::mint::CapShot::One => CapShot::One,
            crate::control::cap::mint::CapShot::Many => CapShot::Many,
        };

        if self.assoc.get_sid(lane) != Some(sid) {
            return Err(CapError::WrongSessionOrLane);
        }

        if kind_tag != K::TAG {
            return Err(CapError::Mismatch);
        }

        // Use nonce-based claim path (trusted domain - no MAC verification)
        let claim_revision = self.next_cap_revision();
        let (exhausted, handle_bytes) = self
            .caps
            .claim_by_nonce(&nonce, sid, lane, kind_tag, role, shot, claim_revision)
            .map_err(|e| match e {
                CapError::UnknownToken => CapError::UnknownToken,
                CapError::WrongSessionOrLane => CapError::WrongSessionOrLane,
                CapError::Exhausted => CapError::Exhausted,
                CapError::TableFull => CapError::TableFull,
                CapError::Mismatch => CapError::Mismatch,
            })?;

        let claim_id = crate::observe::cap_claim::<K>();
        let exhaust_id = crate::observe::cap_exhaust::<K>();

        if exhausted {
            let tap = self.tap();
            emit(
                tap,
                RawEvent::new(self.clock.now32(), exhaust_id)
                    .with_arg0(sid.raw())
                    .with_arg1(0),
            );
        }

        let tap = self.tap();
        emit(
            tap,
            RawEvent::new(self.clock.now32(), claim_id)
                .with_arg0(sid.raw())
                .with_arg1(0),
        );

        let handle = K::decode_handle(handle_bytes).map_err(|_| CapError::Mismatch)?;
        Ok(VerifiedCap::new(handle))
    }

    pub(crate) fn process_topology_intent(
        &self,
        intent: &TopologyIntent,
    ) -> Result<TopologyAck, TopologyError> {
        let dst_rv: RendezvousId = intent.dst_rv;
        let dst_lane: Lane = intent.dst_lane;
        let new_gen: Generation = intent.new_gen;

        // Validate this RV is the intended destination
        if dst_rv != self.id {
            return Err(TopologyError::RendezvousIdMismatch {
                expected: dst_rv,
                got: self.id,
            });
        }

        // Validate lane is in range
        if !self.lane_range.contains(&dst_lane.raw()) {
            return Err(TopologyError::LaneOutOfRange { lane: dst_lane });
        }

        // Check lane is available
        if self.assoc.is_active(dst_lane) {
            return Err(TopologyError::LaneMismatch {
                expected: dst_lane,
                provided: dst_lane,
            });
        }

        // Validate destination-lane generation monotonicity.
        let last_gen = self.r#gen.last(dst_lane).unwrap_or(Generation(0));
        self.validate_topology_generation(dst_lane, new_gen)?;

        // Begin local topology transition using typestate transaction (ack immediately for local state).
        let txn: Txn<LocalTopologyInvariant, IncreasingGen, One> =
            unsafe { Txn::new(dst_lane, last_gen) };
        let mut tap = NoopTap;
        let in_begin = txn.begin(&mut tap);
        let in_acked = in_begin.ack(&mut tap);

        let pending = PendingTopology::destination_prepare(
            SessionId(intent.sid),
            dst_lane,
            self.r#gen.last(dst_lane),
            new_gen,
            in_acked,
            Some((intent.seq_tx, intent.seq_rx)),
        );
        let begin_result = self.topology.begin(dst_lane, pending);
        begin_result?;

        // Create ack using control::automaton::distributed::TopologyAck::new
        let ack = TopologyAck::new(
            intent.src_rv,
            self.id,
            intent.sid,
            new_gen,
            intent.src_lane,
            dst_lane,
            intent.seq_tx,
            intent.seq_rx,
        );

        Ok(ack)
    }

    pub(crate) fn acknowledge_topology_intent(
        &self,
        intent: &TopologyIntent,
    ) -> Result<TopologyAck, TopologyError> {
        let ack = self.process_topology_intent(intent)?;
        self.emit_topology_ack(
            SessionId::new(intent.sid),
            intent.src_lane,
            Lane::new(intent.dst_lane.raw()),
            ack.new_gen,
        );
        Ok(ack)
    }

    fn restore_topology_generation(
        &self,
        lane: Lane,
        previous_generation: Option<Generation>,
    ) -> Result<(), TopologyError> {
        self.r#gen.reset_lane(lane);
        let Some(previous) = previous_generation else {
            return Ok(());
        };
        self.r#gen
            .check_and_update(lane, Generation::ZERO)
            .map_err(|err| match err {
                GenError::StaleOrDuplicate(GenerationRecord { lane, last, new }) => {
                    TopologyError::StaleGeneration { lane, last, new }
                }
                GenError::Overflow { lane, last } => {
                    TopologyError::GenerationOverflow { lane, last }
                }
                GenError::InvalidInitial { lane, new } => {
                    TopologyError::InvalidInitial { lane, new }
                }
            })?;
        if previous != Generation::ZERO {
            self.r#gen
                .restore_to(lane, previous)
                .map_err(|err| match err {
                    GenError::StaleOrDuplicate(GenerationRecord { lane, last, new }) => {
                        TopologyError::StaleGeneration { lane, last, new }
                    }
                    GenError::Overflow { lane, last } => {
                        TopologyError::GenerationOverflow { lane, last }
                    }
                    GenError::InvalidInitial { lane, new } => {
                        TopologyError::InvalidInitial { lane, new }
                    }
                })?;
        }
        Ok(())
    }

    fn commit_prepared_destination_generation(
        &self,
        lane: Lane,
        target: Generation,
    ) -> Result<(), TopologyError> {
        if self.r#gen.last(lane).is_none() {
            let _ = self.r#gen.check_and_update(lane, Generation::ZERO);
        }
        self.r#gen
            .check_and_update(lane, target)
            .map_err(|err| match err {
                GenError::StaleOrDuplicate(GenerationRecord { lane, last, new }) => {
                    TopologyError::StaleGeneration { lane, last, new }
                }
                GenError::Overflow { lane, last } => {
                    TopologyError::GenerationOverflow { lane, last }
                }
                GenError::InvalidInitial { lane, new } => {
                    TopologyError::InvalidInitial { lane, new }
                }
            })
    }

    pub(crate) fn abort_topology_state(&self, sid: SessionId) -> Result<bool, TopologyError> {
        let Some(pending) = self.topology.take_pending_for_sid(sid) else {
            return Ok(false);
        };
        let (_, lane, previous_generation, target, lease_state, state_txn, fences, expected_ack) =
            pending.into_parts();
        let _ = (target, lease_state, state_txn, fences, expected_ack);
        self.topology.reset_lane(lane);
        self.restore_topology_generation(lane, previous_generation)?;
        Ok(true)
    }

    pub(crate) fn state_restore_at_lane(
        &self,
        sid: SessionId,
        lane: Lane,
        epoch: Generation,
    ) -> Result<(), StateRestoreError> {
        match self.eval_effect(
            ControlOp::StateRestore,
            EffectContext::new(sid, lane).with_generation(epoch),
        ) {
            Ok(_) => Ok(()),
            Err(EffectError::StateRestore(err)) => Err(err),
            Err(EffectError::MissingGeneration)
            | Err(EffectError::Unsupported)
            | Err(EffectError::Topology(_))
            | Err(EffectError::Delegation(_))
            | Err(EffectError::TxAbort(_))
            | Err(EffectError::TxCommit(_)) => {
                unreachable!("state restore effect failure is fully covered")
            }
        }
    }

    pub(crate) fn validate_topology_generation(
        &self,
        lane: Lane,
        new_gen: Generation,
    ) -> Result<(), TopologyError> {
        match self.r#gen.last(lane) {
            None => {
                if new_gen.0 >= 1 {
                    Ok(())
                } else {
                    Err(TopologyError::InvalidInitial { lane, new: new_gen })
                }
            }
            Some(prev) if prev.0 == u16::MAX => {
                Err(TopologyError::GenerationOverflow { lane, last: prev })
            }
            Some(prev) if new_gen.0 > prev.0 => Ok(()),
            Some(prev) => Err(TopologyError::StaleGeneration {
                lane,
                last: prev,
                new: new_gen,
            }),
        }
    }
}

// ============================================================================
// Legacy topology helpers have been deleted.
// All topology operations now go through control::CpCommand and EffectRunner.
// The control-plane mini-kernel architecture is responsible for rendezvous access control.

fn map_topology_error(err: TopologyError) -> CpError {
    match err {
        TopologyError::LaneOutOfRange { .. } => {
            CpError::Topology(crate::control::cluster::error::TopologyError::InvalidLane)
        }
        TopologyError::LaneMismatch { .. }
        | TopologyError::InProgress { .. }
        | TopologyError::NoPending { .. }
        | TopologyError::SeqnoMismatch { .. } => {
            CpError::Topology(crate::control::cluster::error::TopologyError::InvalidState)
        }
        TopologyError::UnknownSession { .. } => {
            CpError::Topology(crate::control::cluster::error::TopologyError::InvalidSession)
        }
        TopologyError::StaleGeneration { .. }
        | TopologyError::GenerationOverflow { .. }
        | TopologyError::InvalidInitial { .. } => {
            CpError::Topology(crate::control::cluster::error::TopologyError::GenerationMismatch)
        }
        TopologyError::RemoteRendezvousMismatch { expected, got }
        | TopologyError::RendezvousIdMismatch { expected, got } => CpError::RendezvousMismatch {
            expected: expected.raw(),
            actual: got.raw(),
        },
        TopologyError::PendingTableFull => CpError::ResourceExhausted,
    }
}

#[inline]
fn classify_topology_ack_mismatch(expected: TopologyAck, got: TopologyAck) -> TopologyError {
    if got.sid != expected.sid {
        TopologyError::UnknownSession {
            sid: SessionId::new(got.sid),
        }
    } else if got.src_rv != expected.src_rv {
        TopologyError::RendezvousIdMismatch {
            expected: expected.src_rv,
            got: got.src_rv,
        }
    } else if got.dst_rv != expected.dst_rv {
        TopologyError::RendezvousIdMismatch {
            expected: expected.dst_rv,
            got: got.dst_rv,
        }
    } else if got.src_lane != expected.src_lane {
        TopologyError::LaneMismatch {
            expected: expected.src_lane,
            provided: got.src_lane,
        }
    } else if got.new_lane != expected.new_lane {
        TopologyError::LaneMismatch {
            expected: expected.new_lane,
            provided: got.new_lane,
        }
    } else if got.new_gen != expected.new_gen {
        TopologyError::StaleGeneration {
            lane: expected.new_lane,
            last: expected.new_gen,
            new: got.new_gen,
        }
    } else if got.seq_tx != expected.seq_tx || got.seq_rx != expected.seq_rx {
        TopologyError::SeqnoMismatch {
            seq_tx: got.seq_tx,
            seq_rx: got.seq_rx,
        }
    } else {
        TopologyError::NoPending {
            lane: expected.src_lane,
        }
    }
}

fn map_delegate_error(err: super::error::CapError) -> CpError {
    let deleg_err = match err {
        super::error::CapError::UnknownToken | super::error::CapError::WrongSessionOrLane => {
            crate::control::cluster::error::DelegationError::InvalidToken
        }
        super::error::CapError::Exhausted => {
            crate::control::cluster::error::DelegationError::Exhausted
        }
        super::error::CapError::Mismatch => {
            crate::control::cluster::error::DelegationError::ShotMismatch
        }
        super::error::CapError::TableFull => return CpError::ResourceExhausted,
    };
    CpError::Delegation(deleg_err)
}

fn map_tx_commit_error(err: super::error::TxCommitError) -> CpError {
    match err {
        super::error::TxCommitError::NoStateSnapshot { .. } => {
            CpError::TxCommit(crate::control::cluster::error::TxCommitError::NoStateSnapshot)
        }
        super::error::TxCommitError::AlreadyFinalized { .. } => {
            CpError::TxCommit(crate::control::cluster::error::TxCommitError::AlreadyFinalized)
        }
        super::error::TxCommitError::GenerationMismatch { .. } => {
            CpError::TxCommit(crate::control::cluster::error::TxCommitError::GenerationMismatch)
        }
    }
}

fn map_tx_abort_error(err: super::error::TxAbortError) -> CpError {
    match err {
        super::error::TxAbortError::NoStateSnapshot { .. } => {
            CpError::TxAbort(crate::control::cluster::error::TxAbortError::NoStateSnapshot)
        }
        super::error::TxAbortError::StaleStateSnapshot { .. }
        | super::error::TxAbortError::GenerationMismatch { .. } => {
            CpError::TxAbort(crate::control::cluster::error::TxAbortError::GenerationMismatch)
        }
        super::error::TxAbortError::AlreadyFinalized { .. } => {
            CpError::TxAbort(crate::control::cluster::error::TxAbortError::AlreadyFinalized)
        }
    }
}

fn map_state_restore_error(err: super::error::StateRestoreError) -> CpError {
    match err {
        super::error::StateRestoreError::NoStateSnapshot { .. } => {
            CpError::StateRestore(crate::control::cluster::error::StateRestoreError::EpochNotFound)
        }
        super::error::StateRestoreError::StaleStateSnapshot { .. }
        | super::error::StateRestoreError::EpochMismatch { .. } => {
            CpError::StateRestore(crate::control::cluster::error::StateRestoreError::EpochMismatch)
        }
        super::error::StateRestoreError::AlreadyFinalized { .. } => CpError::StateRestore(
            crate::control::cluster::error::StateRestoreError::AlreadyFinalized,
        ),
    }
}

// ============================================================================
// Local topology operations (used by EffectRunner)
// ============================================================================

impl<'rv, 'cfg, T, U, C, E> Rendezvous<'rv, 'cfg, T, U, C, E>
where
    'cfg: 'rv,
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
    /// Begin a local topology operation.
    ///
    /// This is called by EffectRunner::run_effect() for ControlOp::TopologyBegin.
    fn topology_begin(
        &self,
        sid: SessionId,
        lane: Lane,
        fences: Option<(u32, u32)>,
        generation: Generation,
        expected_ack: Option<TopologyAck>,
    ) -> Result<(), TopologyError> {
        let ctx = EffectContext::new(sid, lane)
            .with_generation(generation)
            .with_fences(fences)
            .with_expected_topology_ack(expected_ack);

        match self.eval_effect(ControlOp::TopologyBegin, ctx) {
            Ok(_) => Ok(()),
            Err(EffectError::Topology(err)) => Err(err),
            Err(EffectError::MissingGeneration)
            | Err(EffectError::Unsupported)
            | Err(EffectError::TxCommit(_))
            | Err(EffectError::TxAbort(_))
            | Err(EffectError::Delegation(_))
            | Err(EffectError::StateRestore(_)) => {
                unreachable!("topology begin effect failure is fully covered")
            }
        }
    }

    fn topology_begin_from_intent(&self, intent: TopologyIntent) -> Result<(), TopologyError> {
        if self.id != intent.src_rv {
            return Err(TopologyError::RendezvousIdMismatch {
                expected: intent.src_rv,
                got: self.id,
            });
        }

        let sid = SessionId(intent.sid);
        let lane = intent.src_lane;
        self.ensure_authenticated_session_lane(sid, lane)?;
        let current = self.r#gen.last(lane).unwrap_or(Generation::ZERO);
        if current != intent.old_gen {
            return Err(TopologyError::StaleGeneration {
                lane,
                last: current,
                new: intent.new_gen,
            });
        }

        let fences =
            (intent.seq_tx != 0 || intent.seq_rx != 0).then_some((intent.seq_tx, intent.seq_rx));
        self.topology_begin(
            sid,
            lane,
            fences,
            intent.new_gen,
            Some(TopologyAck::from_intent(&intent)),
        )
    }

    pub(crate) fn validate_topology_commit_operands(
        &self,
        sid: SessionId,
        operands: TopologyOperands,
    ) -> Result<Lane, TopologyError> {
        let expected = self.expected_topology_ack(sid)?;
        let got = operands.ack(sid);
        if got != expected {
            return Err(classify_topology_ack_mismatch(expected, got));
        }
        Ok(expected.src_lane)
    }

    pub(crate) fn preflight_destination_topology_commit(
        &self,
        sid: SessionId,
        lane: Lane,
    ) -> Result<(), TopologyError> {
        if self.assoc.is_active(lane) {
            return Err(TopologyError::InProgress { lane });
        }
        self.topology.preflight_commit(lane, sid)
    }

    pub(crate) fn finalize_destination_topology_commit(
        &mut self,
        sid: SessionId,
        lane: Lane,
    ) -> Result<(), TopologyError> {
        self.preflight_destination_topology_commit(sid, lane)?;
        let (previous_generation, target) =
            self.topology.prepared_destination_generation(lane, sid)?;
        self.commit_prepared_destination_generation(lane, target)?;
        if let Err(err) = self.topology.finalize_destination(lane, sid) {
            self.restore_topology_generation(lane, previous_generation)?;
            return Err(err);
        }
        Ok(())
    }

    fn revoke_public_endpoints_for_session(&mut self, sid: SessionId) {
        let this = self as *mut Self;
        let mut released_lanes = [Lane::new(0); u8::MAX as usize + 1];
        let lease_capacity = unsafe { usize::from((*this).endpoint_lease_capacity()) };
        let mut idx = 0usize;
        while idx < lease_capacity {
            let Some((slot, generation)) = (unsafe { (*this).public_endpoint_lease_by_index(idx) })
            else {
                idx += 1;
                continue;
            };
            let Some((offset, len)) = (unsafe { (*this).endpoint_lease_storage(slot, generation) })
            else {
                idx += 1;
                continue;
            };
            let (slab_ptr, slab_len) = unsafe { (*this).slab_ptr_and_len() };
            idx += 1;
            if len == 0 || offset + len > slab_len {
                continue;
            }

            let endpoint = unsafe {
                slab_ptr
                    .add(offset)
                    .cast::<crate::endpoint::kernel::PublicEndpointRevoke>()
            };
            let released = unsafe {
                (*endpoint).revoke_for_session(
                    endpoint.cast::<()>(),
                    sid,
                    released_lanes.as_mut_ptr(),
                    released_lanes.len(),
                )
            };
            if released != 0 {
                unsafe {
                    (*this).release_endpoint_lease(slot, generation);
                }
                let mut released_idx = 0usize;
                while released_idx < released {
                    let owned_lane = released_lanes[released_idx];
                    if let Some(released_sid) = unsafe { (*this).release_lane(owned_lane) } {
                        unsafe {
                            (*this).emit_lane_release(released_sid, owned_lane);
                        }
                    }
                    released_idx += 1;
                }
            }
        }
    }

    fn retire_session_lane(&self, sid: SessionId, lane: Lane) {
        while self.assoc.get_sid(lane) == Some(sid) {
            if let Some(released_sid) = self.release_lane(lane) {
                self.emit_lane_release(released_sid, lane);
                break;
            }
        }
    }

    fn retire_session_lanes(&self, sid: SessionId) {
        while let Some(lane) = self.assoc.find_lane(sid) {
            self.retire_session_lane(sid, lane);
        }
    }

    /// Commit a local topology operation.
    ///
    /// This is called by EffectRunner::run_effect() for ControlOp::TopologyCommit.
    pub(crate) fn topology_commit(
        &mut self,
        sid: SessionId,
        lane: Lane,
    ) -> Result<(), TopologyError> {
        let ctx = EffectContext::new(sid, lane);
        match self.eval_effect(ControlOp::TopologyCommit, ctx) {
            Ok(_) => {
                self.revoke_public_endpoints_for_session(sid);
                self.retire_session_lanes(sid);
                Ok(())
            }
            Err(EffectError::Topology(err)) => Err(err),
            Err(EffectError::MissingGeneration)
            | Err(EffectError::Unsupported)
            | Err(EffectError::Delegation(_))
            | Err(EffectError::StateRestore(_))
            | Err(EffectError::TxAbort(_))
            | Err(EffectError::TxCommit(_)) => {
                unreachable!("topology commit failure is fully covered")
            }
        }
    }

    /// Drain transport telemetry and emit tap events for downstream observers.
    fn flush_transport_events(&self) -> Option<crate::transport::TransportEvent> {
        let tap = self.tap();
        let clock = &self.clock;
        let mut last_loss = None;
        let mut emit_event = |event: crate::transport::TransportEvent| {
            let (arg0, arg1) = event.encode_tap_args();
            if matches!(event.kind, TransportEventKind::Loss) {
                last_loss = Some(event);
            }
            emit(
                tap,
                crate::observe::events::TransportEvent::new(clock.now32(), arg0, arg1),
            );
        };
        self.transport.drain_events(&mut emit_event);
        let metrics_attrs = self.transport.metrics().attrs();
        let snapshot = crate::transport::TransportSnapshot::from_policy_attrs(&metrics_attrs);
        if let Some(payload) = snapshot.encode_tap_metrics() {
            let (arg0, arg1) = payload.primary;
            emit(
                tap,
                crate::observe::events::TransportMetrics::new(clock.now32(), arg0, arg1),
            );
            if let Some((ext0, ext1)) = payload.extension {
                emit(
                    tap,
                    crate::observe::events::TransportMetricsExt::new(clock.now32(), ext0, ext1),
                );
            }
        }
        last_loss
    }
}

impl<'rv, 'cfg, T, U, C, E> EffectRunner for Rendezvous<'rv, 'cfg, T, U, C, E>
where
    'cfg: 'rv,
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
    fn run_effect(&mut self, envelope: CpCommand) -> Result<(), CpError> {
        let envelope = match envelope.effect {
            ControlOp::CapDelegate => envelope.canonicalize_delegate()?,
            ControlOp::TopologyBegin | ControlOp::TopologyAck | ControlOp::TopologyCommit => {
                envelope.canonicalize_topology()?
            }
            _ => envelope,
        };
        if matches!(
            envelope.effect,
            ControlOp::TopologyBegin | ControlOp::TopologyAck | ControlOp::TopologyCommit
        ) {
            return Err(CpError::Topology(
                crate::control::cluster::error::TopologyError::InvalidState,
            ));
        }
        let lane_opt = envelope.lane.map(|lane| Lane::new(lane.raw()));
        let sid_opt = envelope.sid.map(|sid| SessionId::new(sid.raw()));

        let policy_event =
            RawEvent::new(self.clock.now32(), control_op_tap_event_id(envelope.effect))
                .with_arg0(sid_opt.map_or(0, |sid| sid.raw()))
                .with_arg1(lane_opt.map_or(0, |lane| lane.raw()));

        let _ = self.flush_transport_events();
        let policy_attrs = self.transport.metrics().attrs();
        let policy_input = crate::policy_runtime::slot_default_input(
            crate::policy_runtime::PolicySlot::Rendezvous,
        );
        let policy_digest = self.policy_digest(crate::policy_runtime::PolicySlot::Rendezvous);
        let event_hash = crate::policy_runtime::hash_tap_event(&policy_event);
        let signals_input_hash = crate::policy_runtime::hash_policy_input(policy_input);
        let signals_attrs_hash = policy_attrs.hash32();
        let transport_snapshot_hash = crate::policy_runtime::hash_transport_attrs(&policy_attrs);
        let replay_transport = crate::policy_runtime::replay_transport_inputs(&policy_attrs);
        let replay_transport_presence =
            crate::policy_runtime::replay_transport_presence(&policy_attrs);
        let mode_id = crate::policy_runtime::POLICY_MODE_AUDIT_ONLY_TAG;
        self.emit_policy_event_with_arg2(
            ids::POLICY_AUDIT,
            lane_opt,
            policy_digest,
            event_hash,
            signals_input_hash,
        );
        self.emit_policy_event_with_arg2(
            ids::POLICY_AUDIT_EXT,
            lane_opt,
            signals_attrs_hash,
            transport_snapshot_hash,
            ((crate::policy_runtime::slot_tag(crate::policy_runtime::PolicySlot::Rendezvous)
                as u32)
                << 24)
                | ((mode_id as u32) << 16),
        );
        self.emit_policy_event_with_arg2(
            ids::POLICY_REPLAY_EVENT,
            lane_opt,
            policy_event.ts,
            policy_event.id as u32,
            policy_event.arg0,
        );
        self.emit_policy_event_with_arg2(
            ids::POLICY_REPLAY_EVENT_EXT,
            lane_opt,
            policy_event.arg1,
            policy_event.arg2,
            policy_event.causal_key as u32,
        );
        self.emit_policy_event_with_arg2(
            ids::POLICY_REPLAY_INPUT0,
            lane_opt,
            policy_input[0],
            policy_input[1],
            policy_input[2],
        );
        self.emit_policy_event_with_arg2(
            ids::POLICY_REPLAY_INPUT1,
            lane_opt,
            policy_input[3],
            0,
            0,
        );
        self.emit_policy_event_with_arg2(
            ids::POLICY_REPLAY_TRANSPORT0,
            lane_opt,
            replay_transport[0],
            replay_transport[1],
            replay_transport[2],
        );
        self.emit_policy_event_with_arg2(
            ids::POLICY_REPLAY_TRANSPORT1,
            lane_opt,
            replay_transport[3],
            replay_transport_presence as u32,
            0,
        );
        let verdict = crate::policy_runtime::PolicyVerdict::NoEngine;
        let verdict_meta = ((crate::policy_runtime::verdict_tag(verdict) as u32) << 24)
            | ((crate::policy_runtime::verdict_arm(verdict) as u32) << 16);
        self.emit_policy_event_with_arg2(
            ids::POLICY_AUDIT_RESULT,
            lane_opt,
            verdict_meta,
            crate::policy_runtime::verdict_reason(verdict) as u32,
            crate::policy_runtime::POLICY_FUEL_NONE as u32,
        );

        self.perform_effect(envelope)
    }
}

// ============================================================================

#[cfg(test)]
mod epf_tests {
    use super::*;
    use crate::{
        control::cluster::core::{CpCommand, EffectRunner, TopologyOperands},
        control::types::{Lane, SessionId},
        g::{self, Msg, Role},
        global::compiled::lowering::LoweringSummary,
        observe::core::TapEvent,
        runtime::{config::Config, consts::RING_EVENTS},
        transport::{Transport, TransportError, wire::Payload},
    };
    use core::{
        cell::{Cell, UnsafeCell},
        mem::MaybeUninit,
        ptr,
    };
    use std::thread_local;

    struct DummyTransport;

    impl Transport for DummyTransport {
        type Error = TransportError;
        type Tx<'a>
            = ()
        where
            Self: 'a;
        type Rx<'a>
            = ()
        where
            Self: 'a;
        type Metrics = ();

        fn open<'a>(&'a self, _local_role: u8, _session_id: u32) -> (Self::Tx<'a>, Self::Rx<'a>) {
            ((), ())
        }

        fn poll_send<'a, 'f>(
            &'a self,
            _tx: &'a mut Self::Tx<'a>,
            _outgoing: crate::transport::Outgoing<'f>,
            _cx: &mut core::task::Context<'_>,
        ) -> core::task::Poll<Result<(), Self::Error>>
        where
            'a: 'f,
        {
            core::task::Poll::Ready(Ok(()))
        }

        fn poll_recv<'a>(
            &'a self,
            _rx: &'a mut Self::Rx<'a>,
            _cx: &mut core::task::Context<'_>,
        ) -> core::task::Poll<Result<Payload<'a>, Self::Error>> {
            core::task::Poll::Ready(Err(TransportError::Offline))
        }

        fn cancel_send<'a>(&'a self, _tx: &'a mut Self::Tx<'a>) {}

        fn requeue<'a>(&'a self, _rx: &'a mut Self::Rx<'a>) {}

        fn drain_events(&self, _emit: &mut dyn FnMut(crate::transport::TransportEvent)) {}

        fn recv_label_hint<'a>(&'a self, _rx: &'a Self::Rx<'a>) -> Option<u8> {
            None
        }

        fn metrics(&self) -> Self::Metrics {
            ()
        }

        fn apply_pacing_update(&self, _interval_us: u32, _burst_bytes: u16) {}
    }

    struct DropTransport;

    impl Drop for DropTransport {
        fn drop(&mut self) {
            DROP_TRANSPORT_COUNT.with(|count| count.set(count.get().saturating_add(1)));
        }
    }

    impl Transport for DropTransport {
        type Error = TransportError;
        type Tx<'a>
            = ()
        where
            Self: 'a;
        type Rx<'a>
            = ()
        where
            Self: 'a;
        type Metrics = ();

        fn open<'a>(&'a self, _local_role: u8, _session_id: u32) -> (Self::Tx<'a>, Self::Rx<'a>) {
            ((), ())
        }

        fn poll_send<'a, 'f>(
            &'a self,
            _tx: &'a mut Self::Tx<'a>,
            _outgoing: crate::transport::Outgoing<'f>,
            _cx: &mut core::task::Context<'_>,
        ) -> core::task::Poll<Result<(), Self::Error>>
        where
            'a: 'f,
        {
            core::task::Poll::Ready(Ok(()))
        }

        fn poll_recv<'a>(
            &'a self,
            _rx: &'a mut Self::Rx<'a>,
            _cx: &mut core::task::Context<'_>,
        ) -> core::task::Poll<Result<Payload<'a>, Self::Error>> {
            core::task::Poll::Ready(Err(TransportError::Offline))
        }

        fn cancel_send<'a>(&'a self, _tx: &'a mut Self::Tx<'a>) {}

        fn requeue<'a>(&'a self, _rx: &'a mut Self::Rx<'a>) {}

        fn drain_events(&self, _emit: &mut dyn FnMut(crate::transport::TransportEvent)) {}

        fn recv_label_hint<'a>(&'a self, _rx: &'a Self::Rx<'a>) -> Option<u8> {
            None
        }

        fn metrics(&self) -> Self::Metrics {
            ()
        }

        fn apply_pacing_update(&self, _interval_us: u32, _burst_bytes: u16) {}
    }

    struct DropClock;

    impl crate::runtime::config::Clock for DropClock {
        fn now32(&self) -> u32 {
            0
        }
    }

    impl Drop for DropClock {
        fn drop(&mut self) {
            DROP_CLOCK_COUNT.with(|count| count.set(count.get().saturating_add(1)));
        }
    }

    type TestRendezvous = Rendezvous<
        'static,
        'static,
        DummyTransport,
        crate::runtime::consts::DefaultLabelUniverse,
        crate::runtime::config::CounterClock,
        crate::control::cap::mint::EpochTbl,
    >;
    type DropTestRendezvous = Rendezvous<
        'static,
        'static,
        DropTransport,
        crate::runtime::consts::DefaultLabelUniverse,
        DropClock,
        crate::control::cap::mint::EpochTbl,
    >;

    thread_local! {
        static EPF_TEST_TAP: UnsafeCell<[TapEvent; RING_EVENTS]> =
            const { UnsafeCell::new([TapEvent::zero(); RING_EVENTS]) };
        static EPF_TEST_SLAB: UnsafeCell<[u8; 32768]> =
            const { UnsafeCell::new([0u8; 32768]) };
        static EPF_TEST_RENDEZVOUS: UnsafeCell<MaybeUninit<TestRendezvous>> =
            const { UnsafeCell::new(MaybeUninit::uninit()) };
        static IMAGE_TEST_SLAB: UnsafeCell<[u8; 32768]> =
            const { UnsafeCell::new([0u8; 32768]) };
        static IMAGE_TEST_RENDEZVOUS: UnsafeCell<MaybeUninit<TestRendezvous>> =
            const { UnsafeCell::new(MaybeUninit::uninit()) };
        static DROP_TEST_TAP: UnsafeCell<[TapEvent; RING_EVENTS]> =
            const { UnsafeCell::new([TapEvent::zero(); RING_EVENTS]) };
        static DROP_TEST_SLAB: UnsafeCell<[u8; core::mem::size_of::<DropTestRendezvous>()
            + core::mem::align_of::<DropTestRendezvous>()]> =
            const { UnsafeCell::new([0u8; core::mem::size_of::<DropTestRendezvous>()
                + core::mem::align_of::<DropTestRendezvous>()]) };
        static DROP_TRANSPORT_COUNT: Cell<u32> = const { Cell::new(0) };
        static DROP_CLOCK_COUNT: Cell<u32> = const { Cell::new(0) };
    }

    fn reset_drop_counts() {
        DROP_TRANSPORT_COUNT.with(|count| count.set(0));
        DROP_CLOCK_COUNT.with(|count| count.set(0));
    }

    fn drop_counts() -> (u32, u32) {
        let transport = DROP_TRANSPORT_COUNT.with(Cell::get);
        let clock = DROP_CLOCK_COUNT.with(Cell::get);
        (transport, clock)
    }

    fn with_epf_test_rendezvous<R>(f: impl FnOnce(&mut TestRendezvous) -> R) -> R {
        EPF_TEST_TAP.with(|tap| {
            EPF_TEST_SLAB.with(|slab| {
                EPF_TEST_RENDEZVOUS.with(|rendezvous| unsafe {
                    let tap = &mut *tap.get();
                    tap.fill(TapEvent::zero());
                    let slab = &mut *slab.get();
                    slab.fill(0);
                    let config = Config::new(tap, slab).with_lane_range(0..2);
                    let ptr = (*rendezvous.get()).as_mut_ptr();
                    let rv_id = RendezvousId::new(1);
                    TestRendezvous::init_from_config(ptr, rv_id, config, DummyTransport, 0);
                    let result = f(&mut *ptr);
                    ptr::drop_in_place(ptr);
                    result
                })
            })
        })
    }

    fn with_image_test_rendezvous_slots<R>(
        endpoint_slots: usize,
        f: impl FnOnce(&mut TestRendezvous) -> R,
    ) -> R {
        EPF_TEST_TAP.with(|tap| {
            IMAGE_TEST_SLAB.with(|slab| {
                IMAGE_TEST_RENDEZVOUS.with(|rendezvous| unsafe {
                    let tap = &mut *tap.get();
                    tap.fill(TapEvent::zero());
                    let slab = &mut *slab.get();
                    slab.fill(0);
                    let config = Config::new(tap, slab).with_lane_range(0..1);
                    let ptr = (*rendezvous.get()).as_mut_ptr();
                    let rv_id = RendezvousId::new(2);
                    TestRendezvous::init_from_config(
                        ptr,
                        rv_id,
                        config,
                        DummyTransport,
                        endpoint_slots,
                    );
                    let result = f(&mut *ptr);
                    ptr::drop_in_place(ptr);
                    result
                })
            })
        })
    }

    fn with_image_test_rendezvous<R>(f: impl FnOnce(&mut TestRendezvous) -> R) -> R {
        with_image_test_rendezvous_slots(0, f)
    }

    fn with_image_test_rendezvous_public_slots<R>(
        endpoint_slots: usize,
        f: impl FnOnce(&mut TestRendezvous) -> R,
    ) -> R {
        EPF_TEST_TAP.with(|tap| {
            IMAGE_TEST_SLAB.with(|slab| unsafe {
                let tap = &mut *tap.get();
                tap.fill(TapEvent::zero());
                let slab = &mut *slab.get();
                slab.fill(0);
                let config = Config::new(tap, slab).with_lane_range(0..1);
                let rv_id = RendezvousId::new(3);
                let ptr =
                    TestRendezvous::init_in_slab(rv_id, config, DummyTransport, endpoint_slots)
                        .expect("public path rendezvous must fit the shared slab");
                let result = f(&mut *ptr);
                ptr::drop_in_place(ptr);
                result
            })
        })
    }

    #[test]
    fn init_in_slab_failure_drops_transport_and_clock() {
        reset_drop_counts();
        DROP_TEST_TAP.with(|tap| {
            DROP_TEST_SLAB.with(|slab| unsafe {
                let tap = &mut *tap.get();
                tap.fill(TapEvent::zero());
                let slab = &mut *slab.get();
                slab.fill(0);
                let config = Config::new(tap, slab)
                    .with_lane_range(0..1)
                    .with_clock(DropClock);
                let rv = DropTestRendezvous::init_in_slab(
                    RendezvousId::new(91),
                    config,
                    DropTransport,
                    0,
                );
                assert!(
                    rv.is_none(),
                    "undersized slab must fail public-path rendezvous init"
                );
            });
        });
        assert_eq!(
            drop_counts(),
            (1, 1),
            "failed init_in_slab must drop moved transport and clock exactly once"
        );
    }

    #[test]
    fn init_in_slab_auto_failure_drops_transport_and_clock() {
        reset_drop_counts();
        DROP_TEST_TAP.with(|tap| {
            DROP_TEST_SLAB.with(|slab| unsafe {
                let tap = &mut *tap.get();
                tap.fill(TapEvent::zero());
                let slab = &mut *slab.get();
                slab.fill(0);
                let config = Config::new(tap, slab)
                    .with_lane_range(0..1)
                    .with_clock(DropClock);
                let rv = DropTestRendezvous::init_in_slab_auto(
                    RendezvousId::new(92),
                    config,
                    DropTransport,
                    1,
                );
                assert!(
                    rv.is_none(),
                    "undersized slab must fail public-path auto rendezvous init"
                );
            });
        });
        assert_eq!(
            drop_counts(),
            (1, 1),
            "failed init_in_slab_auto must drop moved transport and clock exactly once"
        );
    }

    fn route_summary() -> LoweringSummary {
        let program = g::send::<Role<0>, Role<1>, Msg<11, u32>, 0>();
        program.summary().clone()
    }

    fn route_summary_alt() -> LoweringSummary {
        let program = g::send::<Role<0>, Role<1>, Msg<12, u32>, 0>();
        program.summary().clone()
    }

    #[test]
    fn run_effect_allows_when_caps_present() {
        with_epf_test_rendezvous(|rendezvous| {
            let sid = SessionId::new(2);
            let lane = Lane::new(1);

            let envelope =
                CpCommand::state_snapshot(SessionId::new(sid.raw()), Lane::new(lane.raw()));

            let result = EffectRunner::run_effect(rendezvous, envelope);

            assert!(matches!(result, Err(CpError::StateSnapshot(_))));
        });
    }

    #[test]
    fn abort_begin_run_effect_respects_authenticated_lane() {
        with_epf_test_rendezvous(|rendezvous| {
            let sid = SessionId::new(41);
            let lane_a = Lane::new(0);
            let lane_b = Lane::new(1);

            rendezvous.assoc.register(lane_a, sid);
            rendezvous.assoc.register(lane_b, sid);

            EffectRunner::run_effect(rendezvous, CpCommand::abort_begin(sid, lane_b))
                .expect("abort begin must use the authenticated lane from the control token");

            let mut cursor = 0usize;
            let events = rendezvous
                .tap()
                .events_since(&mut cursor, |event| {
                    (event.id == crate::observe::ids::ABORT_BEGIN).then_some(event)
                })
                .collect::<std::vec::Vec<_>>();

            assert_eq!(events.len(), 1);
            assert_eq!(events[0].arg0, sid.raw());
            assert_eq!(events[0].arg1, lane_b.as_wire() as u32);
        });
    }

    #[test]
    fn effect_taps_for_commit_and_tx_abort_carry_lane_causal_keys() {
        with_epf_test_rendezvous(|rendezvous| {
            let sid = SessionId::new(71);
            let commit_lane = Lane::new(0);
            let abort_lane = Lane::new(1);

            rendezvous.assoc.register(commit_lane, sid);
            rendezvous.assoc.register(abort_lane, sid);
            rendezvous
                .r#gen
                .check_and_update(commit_lane, Generation::ZERO)
                .expect("commit lane zero generation must initialize");
            rendezvous
                .r#gen
                .check_and_update(commit_lane, Generation::new(1))
                .expect("commit lane generation must advance before snapshot");
            let commit_generation = rendezvous.state_snapshot_at_lane(sid, commit_lane);
            rendezvous
                .tx_commit_at_lane(sid, commit_lane, commit_generation)
                .expect("commit lane should finalize the snapshot");

            rendezvous
                .r#gen
                .check_and_update(abort_lane, Generation::ZERO)
                .expect("abort lane zero generation must initialize");
            rendezvous
                .r#gen
                .check_and_update(abort_lane, Generation::new(2))
                .expect("abort lane generation must advance before snapshot");
            let abort_generation = rendezvous.state_snapshot_at_lane(sid, abort_lane);
            rendezvous
                .r#gen
                .check_and_update(abort_lane, Generation::new(4))
                .expect("abort lane generation must advance beyond the snapshot");
            rendezvous
                .tx_abort_at_lane(sid, abort_lane, abort_generation)
                .expect("abort lane should restore the snapshot generation");

            let mut cursor = 0usize;
            let events = rendezvous
                .tap()
                .events_since(&mut cursor, |event| match event.id {
                    crate::observe::ids::POLICY_COMMIT | crate::observe::ids::POLICY_TX_ABORT => {
                        Some(event)
                    }
                    _ => None,
                })
                .collect::<std::vec::Vec<_>>();

            assert_eq!(
                events.len(),
                2,
                "expected one commit tap and one tx-abort tap"
            );

            let commit = events
                .iter()
                .find(|event| event.id == crate::observe::ids::POLICY_COMMIT)
                .copied()
                .expect("commit tap");
            assert_eq!(commit.arg0, sid.raw());
            assert_eq!(commit.arg1, commit_generation.0 as u32);
            assert_eq!(
                commit.causal_key,
                TapEvent::make_causal_key(commit_lane.as_wire() + 1, 0),
                "commit tap must encode the originating lane in its causal key"
            );

            let tx_abort = events
                .iter()
                .find(|event| event.id == crate::observe::ids::POLICY_TX_ABORT)
                .copied()
                .expect("tx abort tap");
            assert_eq!(tx_abort.arg0, sid.raw());
            assert_eq!(tx_abort.arg1, abort_generation.0 as u32);
            assert_eq!(
                tx_abort.causal_key,
                TapEvent::make_causal_key(abort_lane.as_wire() + 1, 0),
                "tx-abort tap must encode the originating lane in its causal key"
            );
        });
    }

    #[test]
    fn topology_begin_run_effect_rejects_direct_begin_before_mutation() {
        with_epf_test_rendezvous(|rendezvous| {
            let sid = SessionId::new(42);
            let src_lane = Lane::new(0);
            let dst_lane = Lane::new(1);

            rendezvous
                .prepare_topology_control_scope(src_lane)
                .expect("topology tests must bind topology storage");
            rendezvous.assoc.register(src_lane, sid);

            let operands = TopologyOperands::new(
                rendezvous.id,
                RendezvousId::new(9),
                src_lane,
                dst_lane,
                Generation::ZERO,
                Generation::new(1),
                11,
                13,
            );
            assert!(matches!(
                EffectRunner::run_effect(rendezvous, CpCommand::topology_begin(sid, operands)),
                Err(CpError::Topology(
                    crate::control::cluster::error::TopologyError::InvalidState
                ))
            ));

            rendezvous.topology_begin_from_intent(operands.intent(sid)).expect(
                "direct topology begin rejection must not wedge the cluster-owned topology path",
            );
        });
    }

    #[test]
    fn topology_begin_run_effect_rejects_internal_lane_split_before_mutation() {
        with_epf_test_rendezvous(|rendezvous| {
            let sid = SessionId::new(420);
            let authenticated_lane = Lane::new(0);
            let wrong_lane = Lane::new(1);
            let dst_lane = Lane::new(2);

            rendezvous
                .prepare_topology_control_scope(authenticated_lane)
                .expect("topology tests must bind topology storage");
            rendezvous
                .prepare_topology_control_scope(wrong_lane)
                .expect("topology tests must bind topology storage");
            rendezvous.assoc.register(authenticated_lane, sid);

            let operands = TopologyOperands::new(
                rendezvous.id,
                RendezvousId::new(9),
                authenticated_lane,
                dst_lane,
                Generation::ZERO,
                Generation::new(1),
                5,
                7,
            );
            let malformed = CpCommand::new(ControlOp::TopologyBegin)
                .with_sid(sid)
                .with_lane(wrong_lane)
                .with_topology(operands);

            assert!(matches!(
                EffectRunner::run_effect(rendezvous, malformed),
                Err(CpError::Topology(
                    crate::control::cluster::error::TopologyError::LaneMismatch
                ))
            ));

            assert!(matches!(
                EffectRunner::run_effect(rendezvous, CpCommand::topology_begin(sid, operands)),
                Err(CpError::Topology(
                    crate::control::cluster::error::TopologyError::InvalidState
                ))
            ));

            rendezvous
                .topology_begin_from_intent(operands.intent(sid))
                .expect("rejected direct begin must not wedge the cluster-owned topology path");
        });
    }

    #[test]
    fn topology_begin_from_intent_rejects_foreign_source_rendezvous_before_mutation() {
        with_epf_test_rendezvous(|rendezvous| {
            let sid = SessionId::new(421);
            let src_lane = Lane::new(0);
            let dst_lane = Lane::new(1);
            let foreign_src = RendezvousId::new(rendezvous.id.raw().saturating_add(1));

            rendezvous
                .prepare_topology_control_scope(src_lane)
                .expect("topology tests must bind topology storage");
            rendezvous.assoc.register(src_lane, sid);

            let invalid = TopologyOperands::new(
                foreign_src,
                RendezvousId::new(9),
                src_lane,
                dst_lane,
                Generation::ZERO,
                Generation::new(1),
                23,
                29,
            );
            assert!(matches!(
                rendezvous.topology_begin_from_intent(invalid.intent(sid)),
                Err(TopologyError::RendezvousIdMismatch { expected, got })
                    if expected == foreign_src && got == rendezvous.id
            ));

            let valid = TopologyOperands::new(
                rendezvous.id,
                RendezvousId::new(9),
                src_lane,
                dst_lane,
                Generation::ZERO,
                Generation::new(1),
                23,
                29,
            );
            rendezvous
                .topology_begin_from_intent(valid.intent(sid))
                .expect("failed begin preflight must not wedge the topology intent path");
        });
    }

    #[test]
    fn topology_begin_from_intent_rejects_stale_old_generation_before_mutation() {
        with_epf_test_rendezvous(|rendezvous| {
            let sid = SessionId::new(422);
            let src_lane = Lane::new(0);
            let dst_lane = Lane::new(1);

            rendezvous
                .prepare_topology_control_scope(src_lane)
                .expect("topology tests must bind topology storage");
            rendezvous.assoc.register(src_lane, sid);
            rendezvous.advance_lane_generation_for_test(src_lane, Generation::new(1));

            let stale = TopologyOperands::new(
                rendezvous.id,
                RendezvousId::new(10),
                src_lane,
                dst_lane,
                Generation::ZERO,
                Generation::new(2),
                31,
                37,
            );
            assert!(matches!(
                rendezvous.topology_begin_from_intent(stale.intent(sid)),
                Err(TopologyError::StaleGeneration { lane, last, new })
                    if lane == src_lane
                        && last == Generation::new(1)
                        && new == Generation::new(2)
            ));

            let valid = TopologyOperands::new(
                rendezvous.id,
                RendezvousId::new(10),
                src_lane,
                dst_lane,
                Generation::new(1),
                Generation::new(2),
                31,
                37,
            );
            rendezvous
                .topology_begin_from_intent(valid.intent(sid))
                .expect("stale rejection must leave the topology intent path reusable");
        });
    }

    #[test]
    fn topology_begin_run_effect_rejects_operandless_command() {
        with_epf_test_rendezvous(|rendezvous| {
            let sid = SessionId::new(423);
            let lane = Lane::new(0);

            assert_eq!(
                EffectRunner::run_effect(
                    rendezvous,
                    CpCommand::new(ControlOp::TopologyBegin)
                        .with_sid(sid)
                        .with_lane(lane)
                        .with_generation(Generation::new(1)),
                ),
                Err(CpError::Topology(
                    crate::control::cluster::error::TopologyError::InvalidState,
                ))
            );
        });
    }

    #[test]
    fn topology_begin_from_intent_rejects_unauthenticated_source_lane() {
        with_epf_test_rendezvous(|rendezvous| {
            let sid = SessionId::new(43);
            let authenticated_lane = Lane::new(0);
            let wrong_lane = Lane::new(1);
            let dst_lane = Lane::new(2);

            rendezvous
                .prepare_topology_control_scope(authenticated_lane)
                .expect("topology tests must bind topology storage");
            rendezvous
                .prepare_topology_control_scope(wrong_lane)
                .expect("topology tests must bind topology storage");
            rendezvous.assoc.register(authenticated_lane, sid);

            let invalid = TopologyIntent::new(
                rendezvous.id,
                RendezvousId::new(7),
                sid.raw(),
                Generation::ZERO,
                Generation::new(1),
                17,
                19,
                wrong_lane,
                dst_lane,
            );
            assert!(matches!(
                rendezvous.topology_begin_from_intent(invalid),
                Err(TopologyError::UnknownSession { sid: err_sid }) if err_sid == sid
            ));

            let valid = TopologyIntent::new(
                rendezvous.id,
                RendezvousId::new(7),
                sid.raw(),
                Generation::ZERO,
                Generation::new(1),
                17,
                19,
                authenticated_lane,
                dst_lane,
            );
            rendezvous
                .topology_begin_from_intent(valid)
                .expect("authenticated lane must remain usable after rejected begin intent");
        });
    }

    #[test]
    fn topology_begin_rejects_duplicate_pending_session_across_lanes() {
        with_epf_test_rendezvous(|rendezvous| {
            let sid = SessionId::new(45);
            let lane_a = Lane::new(0);
            let lane_b = Lane::new(1);
            let dst_a = Lane::new(2);
            let dst_b = Lane::new(3);
            let first = TopologyOperands::new(
                rendezvous.id,
                RendezvousId::new(9),
                lane_a,
                dst_a,
                Generation::ZERO,
                Generation::new(1),
                11,
                13,
            );
            let second = TopologyOperands::new(
                rendezvous.id,
                RendezvousId::new(10),
                lane_b,
                dst_b,
                Generation::ZERO,
                Generation::new(1),
                17,
                19,
            );

            rendezvous
                .prepare_topology_control_scope(lane_a)
                .expect("topology tests must bind topology storage");
            rendezvous
                .prepare_topology_control_scope(lane_b)
                .expect("topology tests must bind topology storage");
            rendezvous.assoc.register(lane_a, sid);
            rendezvous.assoc.register(lane_b, sid);

            rendezvous
                .topology_begin(
                    sid,
                    lane_a,
                    Some((first.seq_tx, first.seq_rx)),
                    first.new_gen,
                    Some(first.ack(sid)),
                )
                .expect("first begin must succeed");

            assert_eq!(
                rendezvous.topology_begin(
                    sid,
                    lane_b,
                    Some((second.seq_tx, second.seq_rx)),
                    second.new_gen,
                    Some(second.ack(sid)),
                ),
                Err(TopologyError::InProgress { lane: lane_a })
            );
            assert_eq!(
                rendezvous.expected_topology_ack(sid),
                Ok(first.ack(sid)),
                "duplicate begin rejection must keep the canonical expected ACK bound to the first lane"
            );
            assert_eq!(
                rendezvous.topology.topology_commit(lane_b, sid),
                Err(TopologyError::NoPending { lane: lane_b }),
                "duplicate begin rejection must leave the second lane untouched"
            );
        });
    }

    #[test]
    fn topology_commit_run_effect_is_cluster_owned_and_preserves_pending_state() {
        with_epf_test_rendezvous(|rendezvous| {
            let sid = SessionId::new(46);
            let src_lane = Lane::new(0);
            let dst_lane = Lane::new(1);
            let expected = TopologyOperands::new(
                rendezvous.id,
                RendezvousId::new(11),
                src_lane,
                dst_lane,
                Generation::ZERO,
                Generation::new(1),
                41,
                43,
            );

            rendezvous
                .prepare_topology_control_scope(src_lane)
                .expect("topology tests must bind topology storage");
            rendezvous.assoc.register(src_lane, sid);
            rendezvous
                .topology_begin_from_intent(expected.intent(sid))
                .expect("begin effect");

            assert!(matches!(
                EffectRunner::run_effect(rendezvous, CpCommand::topology_commit(sid, expected)),
                Err(CpError::Topology(
                    crate::control::cluster::error::TopologyError::InvalidState
                ))
            ));
            assert_eq!(
                rendezvous.expected_topology_ack(sid),
                Ok(expected.ack(sid)),
                "direct commit rejection must preserve the source-side expected ACK"
            );
            assert_eq!(
                rendezvous.session_lane(sid),
                Some(src_lane),
                "direct commit rejection must not retire the authenticated source lane"
            );
        });
    }

    #[test]
    fn topology_commit_run_effect_rejects_operandless_command_before_mutation() {
        with_epf_test_rendezvous(|rendezvous| {
            let sid = SessionId::new(47);
            let src_lane = Lane::new(0);
            let dst_lane = Lane::new(1);
            let expected = TopologyOperands::new(
                rendezvous.id,
                RendezvousId::new(12),
                src_lane,
                dst_lane,
                Generation::ZERO,
                Generation::new(1),
                47,
                53,
            );

            rendezvous
                .prepare_topology_control_scope(src_lane)
                .expect("topology tests must bind topology storage");
            rendezvous.assoc.register(src_lane, sid);
            rendezvous
                .topology_begin_from_intent(expected.intent(sid))
                .expect("begin effect");

            assert_eq!(
                EffectRunner::run_effect(
                    rendezvous,
                    CpCommand::new(ControlOp::TopologyCommit)
                        .with_sid(sid)
                        .with_lane(src_lane),
                ),
                Err(CpError::Topology(
                    crate::control::cluster::error::TopologyError::InvalidState,
                ))
            );
            assert_eq!(
                rendezvous.expected_topology_ack(sid),
                Ok(expected.ack(sid)),
                "operand-less direct commit rejection must preserve the canonical expected ACK",
            );
            assert_eq!(
                rendezvous.session_lane(sid),
                Some(src_lane),
                "operand-less direct commit rejection must not retire the authenticated source lane",
            );
        });
    }

    #[test]
    fn state_snapshot_run_effect_respects_authenticated_lane() {
        with_epf_test_rendezvous(|rendezvous| {
            let sid = SessionId::new(44);
            let lane_a = Lane::new(0);
            let lane_b = Lane::new(1);

            rendezvous.assoc.register(lane_a, sid);
            rendezvous.assoc.register(lane_b, sid);
            rendezvous
                .r#gen
                .check_and_update(lane_a, Generation::ZERO)
                .expect("lane A zero generation must initialize");
            rendezvous
                .r#gen
                .check_and_update(lane_a, Generation::new(1))
                .expect("lane A generation must advance");
            rendezvous
                .r#gen
                .check_and_update(lane_b, Generation::ZERO)
                .expect("lane B zero generation must initialize");
            rendezvous
                .r#gen
                .check_and_update(lane_b, Generation::new(3))
                .expect("lane B generation must advance");

            EffectRunner::run_effect(rendezvous, CpCommand::state_snapshot(sid, lane_b))
                .expect("state snapshot must target the lane authenticated by the token");

            assert_eq!(rendezvous.state_snapshots.last_snapshot(lane_a), None);
            assert_eq!(
                rendezvous.state_snapshots.last_snapshot(lane_b),
                Some(Generation::new(3))
            );
        });
    }

    #[test]
    fn claim_cap_rejects_malformed_endpoint_control_header() {
        with_epf_test_rendezvous(|rendezvous| {
            let sid = SessionId::new(7);
            let lane = Lane::new(1);
            let role = 5;
            let nonce = [0xCD; crate::control::cap::mint::CAP_NONCE_LEN];
            let handle = crate::control::cap::mint::EndpointHandle::new(sid, lane, role);

            let mut header = [0u8; crate::control::cap::mint::CAP_HEADER_LEN];
            crate::control::cap::mint::CapHeader::new(
                sid,
                lane,
                role,
                crate::control::cap::mint::EndpointResource::TAG,
                0,
                ControlOp::Fence,
                crate::control::cap::mint::ControlPath::Local,
                crate::control::cap::mint::CapShot::One,
                crate::global::const_dsl::ControlScopeKind::None,
                0,
                0,
                0,
                crate::control::cap::mint::EndpointResource::encode_handle(&handle),
            )
            .encode(&mut header);
            header[13] = 0x80;

            let token = crate::control::cap::mint::GenericCapToken::<
                crate::control::cap::mint::EndpointResource,
            >::from_parts(
                nonce, header, [0; crate::control::cap::mint::CAP_TAG_LEN]
            );

            assert!(matches!(
                rendezvous.claim_cap(&token),
                Err(CapError::Mismatch)
            ));
        });
    }

    #[test]
    fn claim_cap_rejects_malformed_endpoint_handle_payload() {
        with_epf_test_rendezvous(|rendezvous| {
            let sid = SessionId::new(7);
            let lane = Lane::new(1);
            let role = 5;
            let nonce = [0xCE; crate::control::cap::mint::CAP_NONCE_LEN];
            let handle = crate::control::cap::mint::EndpointHandle::new(sid, lane, role);

            rendezvous.assoc.register(lane, sid);
            rendezvous
                .ensure_endpoint_resident_budget(EndpointResidentBudget::with_route_storage(
                    0, 0, 0, 1,
                ))
                .expect("claim test must bind cap storage");
            rendezvous
                .mint_cap::<crate::control::cap::mint::EndpointResource>(
                    sid,
                    lane,
                    crate::control::cap::mint::CapShot::One,
                    role,
                    nonce,
                    handle,
                )
                .expect("valid capability mint must succeed");

            let mut header = [0u8; crate::control::cap::mint::CAP_HEADER_LEN];
            crate::control::cap::mint::CapHeader::new(
                sid,
                lane,
                role,
                crate::control::cap::mint::EndpointResource::TAG,
                0,
                ControlOp::Fence,
                crate::control::cap::mint::ControlPath::Local,
                crate::control::cap::mint::CapShot::One,
                crate::global::const_dsl::ControlScopeKind::None,
                0,
                0,
                0,
                crate::control::cap::mint::EndpointResource::encode_handle(&handle),
            )
            .encode(&mut header);
            header[crate::control::cap::mint::CAP_CONTROL_HEADER_FIXED_LEN + 6] = 0x7F;

            let token = crate::control::cap::mint::GenericCapToken::<
                crate::control::cap::mint::EndpointResource,
            >::from_parts(
                nonce, header, [0; crate::control::cap::mint::CAP_TAG_LEN]
            );

            assert!(matches!(
                rendezvous.claim_cap(&token),
                Err(CapError::Mismatch)
            ));
        });
    }

    #[test]
    fn delegate_and_claim_reject_noncanonical_decodable_endpoint_headers() {
        fn endpoint_token_with_mutated_header(
            mutate: fn(&mut [u8; crate::control::cap::mint::CAP_HEADER_LEN]),
        ) -> crate::control::cap::mint::GenericCapToken<crate::control::cap::mint::EndpointResource>
        {
            let sid = SessionId::new(7);
            let lane = Lane::new(1);
            let role = 5;
            let handle = crate::control::cap::mint::EndpointHandle::new(sid, lane, role);
            let mut header = [0u8; crate::control::cap::mint::CAP_HEADER_LEN];
            crate::control::cap::mint::CapHeader::new(
                sid,
                lane,
                role,
                crate::control::cap::mint::EndpointResource::TAG,
                0,
                ControlOp::Fence,
                crate::control::cap::mint::ControlPath::Local,
                crate::control::cap::mint::CapShot::One,
                crate::global::const_dsl::ControlScopeKind::None,
                0,
                0,
                0,
                crate::control::cap::mint::EndpointResource::encode_handle(&handle),
            )
            .encode(&mut header);
            mutate(&mut header);

            crate::control::cap::mint::GenericCapToken::<
                crate::control::cap::mint::EndpointResource,
            >::from_parts(
                [0xCD; crate::control::cap::mint::CAP_NONCE_LEN],
                header,
                [0; crate::control::cap::mint::CAP_TAG_LEN],
            )
        }

        fn mutate_tag(header: &mut [u8; crate::control::cap::mint::CAP_HEADER_LEN]) {
            header[7] = crate::control::cap::resource_kinds::LoopContinueKind::TAG;
        }

        fn mutate_label(header: &mut [u8; crate::control::cap::mint::CAP_HEADER_LEN]) {
            header[8] = 1;
        }

        fn mutate_op(header: &mut [u8; crate::control::cap::mint::CAP_HEADER_LEN]) {
            header[9] = ControlOp::TopologyBegin.as_u8();
        }

        fn mutate_path(header: &mut [u8; crate::control::cap::mint::CAP_HEADER_LEN]) {
            header[10] = crate::control::cap::mint::ControlPath::Wire.as_u8();
        }

        fn mutate_shot(header: &mut [u8; crate::control::cap::mint::CAP_HEADER_LEN]) {
            header[11] = crate::control::cap::mint::CapShot::Many.as_u8();
        }

        fn mutate_scope_kind(header: &mut [u8; crate::control::cap::mint::CAP_HEADER_LEN]) {
            header[12] = crate::global::const_dsl::ControlScopeKind::Route as u8;
        }

        fn mutate_flags(header: &mut [u8; crate::control::cap::mint::CAP_HEADER_LEN]) {
            header[13] = 0x01;
        }

        fn mutate_scope_id(header: &mut [u8; crate::control::cap::mint::CAP_HEADER_LEN]) {
            header[14..16].copy_from_slice(&1u16.to_be_bytes());
        }

        fn mutate_epoch(header: &mut [u8; crate::control::cap::mint::CAP_HEADER_LEN]) {
            header[16..18].copy_from_slice(&1u16.to_be_bytes());
        }

        let cases: &[(
            &str,
            fn(&mut [u8; crate::control::cap::mint::CAP_HEADER_LEN]),
        )] = &[
            ("tag", mutate_tag),
            ("label", mutate_label),
            ("op", mutate_op),
            ("path", mutate_path),
            ("shot", mutate_shot),
            ("scope_kind", mutate_scope_kind),
            ("flags", mutate_flags),
            ("scope_id", mutate_scope_id),
            ("epoch", mutate_epoch),
        ];

        with_epf_test_rendezvous(|rendezvous| {
            rendezvous.assoc.register(Lane::new(1), SessionId::new(7));
            for (name, mutate) in cases {
                let token = endpoint_token_with_mutated_header(*mutate);
                assert!(
                    token.control_header().is_ok(),
                    "{name} mutation must remain decodable to exercise canonical validation",
                );

                let envelope = CpCommand::new(ControlOp::CapDelegate).with_delegate(
                    crate::control::cluster::core::DelegateOperands {
                        claim: false,
                        token,
                    },
                );
                assert!(
                    matches!(
                        EffectRunner::run_effect(rendezvous, envelope),
                        Err(CpError::Delegation(_))
                    ),
                    "{name} mutation must be rejected by delegate execution",
                );
                assert!(
                    matches!(rendezvous.claim_cap(&token), Err(CapError::Mismatch)),
                    "{name} mutation must be rejected by claim_cap",
                );
            }
        });
    }

    #[test]
    fn cap_delegate_rejects_unregistered_lane_without_panicking() {
        with_epf_test_rendezvous(|rendezvous| {
            let sid = SessionId::new(7);
            let lane = Lane::new(1);
            let role = 5;
            let nonce = [0xD1; crate::control::cap::mint::CAP_NONCE_LEN];
            let handle = crate::control::cap::mint::EndpointHandle::new(sid, lane, role);

            let mut header = [0u8; crate::control::cap::mint::CAP_HEADER_LEN];
            crate::control::cap::mint::CapHeader::new(
                sid,
                lane,
                role,
                crate::control::cap::mint::EndpointResource::TAG,
                0,
                ControlOp::Fence,
                crate::control::cap::mint::ControlPath::Local,
                crate::control::cap::mint::CapShot::One,
                crate::global::const_dsl::ControlScopeKind::None,
                0,
                0,
                0,
                crate::control::cap::mint::EndpointResource::encode_handle(&handle),
            )
            .encode(&mut header);
            let token = crate::control::cap::mint::GenericCapToken::<
                crate::control::cap::mint::EndpointResource,
            >::from_parts(
                nonce, header, [0; crate::control::cap::mint::CAP_TAG_LEN]
            );

            let envelope = CpCommand::new(ControlOp::CapDelegate).with_delegate(
                crate::control::cluster::core::DelegateOperands {
                    claim: false,
                    token,
                },
            );

            assert!(matches!(
                EffectRunner::run_effect(rendezvous, envelope),
                Err(CpError::Delegation(
                    crate::control::cluster::error::DelegationError::InvalidToken
                ))
            ));
        });
    }

    #[test]
    fn cap_delegate_reports_resource_exhaustion_when_cap_table_is_full() {
        with_epf_test_rendezvous(|rendezvous| {
            let sid = SessionId::new(7);
            let lane = Lane::new(1);
            let role = 5;

            rendezvous.assoc.register(lane, sid);
            rendezvous
                .ensure_endpoint_resident_budget(EndpointResidentBudget::with_route_storage(
                    0, 0, 0, 1,
                ))
                .expect("delegate test must bind one cap entry");

            let make_token = |nonce| {
                let handle = crate::control::cap::mint::EndpointHandle::new(sid, lane, role);
                let mut header = [0u8; crate::control::cap::mint::CAP_HEADER_LEN];
                crate::control::cap::mint::CapHeader::new(
                    sid,
                    lane,
                    role,
                    crate::control::cap::mint::EndpointResource::TAG,
                    0,
                    ControlOp::Fence,
                    crate::control::cap::mint::ControlPath::Local,
                    crate::control::cap::mint::CapShot::One,
                    crate::global::const_dsl::ControlScopeKind::None,
                    0,
                    0,
                    0,
                    crate::control::cap::mint::EndpointResource::encode_handle(&handle),
                )
                .encode(&mut header);
                crate::control::cap::mint::GenericCapToken::<
                    crate::control::cap::mint::EndpointResource,
                >::from_parts(
                    nonce, header, [0; crate::control::cap::mint::CAP_TAG_LEN]
                )
            };

            let first = CpCommand::new(ControlOp::CapDelegate).with_delegate(
                crate::control::cluster::core::DelegateOperands {
                    claim: false,
                    token: make_token([0xD2; crate::control::cap::mint::CAP_NONCE_LEN]),
                },
            );
            EffectRunner::run_effect(rendezvous, first)
                .expect("first delegate mint must consume the only cap slot");

            let second = CpCommand::new(ControlOp::CapDelegate).with_delegate(
                crate::control::cluster::core::DelegateOperands {
                    claim: false,
                    token: make_token([0xD3; crate::control::cap::mint::CAP_NONCE_LEN]),
                },
            );
            assert!(matches!(
                EffectRunner::run_effect(rendezvous, second),
                Err(CpError::ResourceExhausted)
            ));
        });
    }

    #[test]
    fn state_restore_rewinds_generation_to_recorded_snapshot() {
        with_epf_test_rendezvous(|rendezvous| {
            let sid = SessionId::new(7);
            let lane = Lane::new(1);

            rendezvous.assoc.register(lane, sid);
            rendezvous
                .r#gen
                .check_and_update(lane, Generation::ZERO)
                .expect("lane zero generation must initialize");
            rendezvous
                .r#gen
                .check_and_update(lane, Generation::new(1))
                .expect("generation must advance before snapshot");
            let snapshot = rendezvous.state_snapshot_at_lane(sid, lane);
            assert_eq!(snapshot, Generation::new(1));

            rendezvous
                .r#gen
                .check_and_update(lane, Generation::new(3))
                .expect("generation must advance beyond snapshot");
            assert_eq!(rendezvous.r#gen.last(lane), Some(Generation::new(3)));

            rendezvous
                .state_restore_at_lane(sid, lane, snapshot)
                .expect("recorded snapshot must restore lane generation");

            assert_eq!(rendezvous.r#gen.last(lane), Some(snapshot));
            assert_eq!(
                rendezvous.state_snapshots.finalization(lane),
                Some(SnapshotFinalization::Restored)
            );
        });
    }

    #[test]
    fn state_restore_run_effect_respects_authenticated_lane() {
        with_epf_test_rendezvous(|rendezvous| {
            let sid = SessionId::new(43);
            let lane_a = Lane::new(0);
            let lane_b = Lane::new(1);

            rendezvous.assoc.register(lane_a, sid);
            rendezvous.assoc.register(lane_b, sid);

            rendezvous
                .r#gen
                .check_and_update(lane_a, Generation::ZERO)
                .expect("lane A zero generation must initialize");
            rendezvous
                .r#gen
                .check_and_update(lane_a, Generation::new(1))
                .expect("lane A generation must advance");
            let snapshot_a = rendezvous.state_snapshot_at_lane(sid, lane_a);

            rendezvous
                .r#gen
                .check_and_update(lane_b, Generation::ZERO)
                .expect("lane B zero generation must initialize");
            rendezvous
                .r#gen
                .check_and_update(lane_b, Generation::new(3))
                .expect("lane B generation must advance before snapshot");
            let snapshot_b = rendezvous.state_snapshot_at_lane(sid, lane_b);
            rendezvous
                .r#gen
                .check_and_update(lane_b, Generation::new(5))
                .expect("lane B generation must advance beyond the snapshot");

            EffectRunner::run_effect(
                rendezvous,
                CpCommand::state_restore(sid, lane_b, snapshot_b),
            )
            .expect("state restore must target the lane authenticated by the token");

            assert_eq!(rendezvous.r#gen.last(lane_a), Some(snapshot_a));
            assert_eq!(rendezvous.r#gen.last(lane_b), Some(snapshot_b));
            assert_eq!(
                rendezvous.state_snapshots.finalization(lane_b),
                Some(SnapshotFinalization::Restored)
            );
        });
    }

    #[test]
    fn state_restore_clears_pending_topology_from_newer_epoch() {
        with_epf_test_rendezvous(|rendezvous| {
            let sid = SessionId::new(13);
            let lane = Lane::new(1);
            let fences = Some((17, 23));
            let pending_generation = Generation::new(2);

            rendezvous
                .prepare_topology_control_scope(lane)
                .expect("topology tests must bind topology storage");
            rendezvous.assoc.register(lane, sid);
            rendezvous
                .r#gen
                .check_and_update(lane, Generation::ZERO)
                .expect("lane zero generation must initialize");
            rendezvous
                .r#gen
                .check_and_update(lane, Generation::new(1))
                .expect("generation must advance before snapshot");

            let snapshot = rendezvous.state_snapshot_at_lane(sid, lane);
            let expected_ack = TopologyAck::new(
                rendezvous.id,
                RendezvousId::new(99),
                sid.raw(),
                pending_generation,
                lane,
                Lane::new(2),
                17,
                23,
            );
            rendezvous
                .topology_begin(sid, lane, fences, pending_generation, Some(expected_ack))
                .expect("topology begin must stage pending topology state");

            rendezvous
                .state_restore_at_lane(sid, lane, snapshot)
                .expect("restore must clear transient topology state recorded after snapshot");

            rendezvous
                .topology_begin(sid, lane, fences, pending_generation, Some(expected_ack))
                .expect("restored lane must accept a fresh topology begin");
        });
    }

    #[test]
    fn process_topology_intent_leaves_no_pending_state_on_generation_failure() {
        with_epf_test_rendezvous(|rendezvous| {
            let sid = SessionId::new(32);
            let lane = Lane::new(1);

            rendezvous
                .prepare_topology_control_scope(lane)
                .expect("topology tests must bind topology storage");
            rendezvous
                .r#gen
                .check_and_update(lane, Generation::ZERO)
                .expect("lane zero generation must initialize");
            rendezvous
                .r#gen
                .check_and_update(lane, Generation::new(1))
                .expect("generation must advance before validating stale intent");

            let stale = TopologyIntent::new(
                RendezvousId::new(7),
                rendezvous.id,
                sid.raw(),
                Generation::new(1),
                Generation::new(1),
                11,
                13,
                Lane::new(0),
                lane,
            );
            assert!(matches!(
                rendezvous.process_topology_intent(&stale),
                Err(TopologyError::StaleGeneration { lane: err_lane, .. }) if err_lane == lane
            ));

            let valid = TopologyIntent::new(
                RendezvousId::new(7),
                rendezvous.id,
                sid.raw(),
                Generation::new(1),
                Generation::new(2),
                11,
                13,
                Lane::new(0),
                lane,
            );
            rendezvous
                .process_topology_intent(&valid)
                .expect("stale intent must not leave pending topology wedged on the lane");
        });
    }

    #[test]
    fn process_topology_intent_accepts_established_source_generation_on_fresh_destination_lane() {
        with_epf_test_rendezvous(|rendezvous| {
            let sid = SessionId::new(33);
            let dst_lane = Lane::new(1);

            rendezvous
                .prepare_topology_control_scope(dst_lane)
                .expect("topology tests must bind topology storage");

            let intent = TopologyIntent::new(
                RendezvousId::new(7),
                rendezvous.id,
                sid.raw(),
                Generation::new(5),
                Generation::new(6),
                3,
                7,
                Lane::new(0),
                dst_lane,
            );
            let ack = rendezvous
                .process_topology_intent(&intent)
                .expect("fresh destination lane must not reject an established source generation");

            assert_eq!(ack, TopologyAck::from_intent(&intent));
            assert_eq!(
                rendezvous.lane_generation(dst_lane),
                Generation::ZERO,
                "destination prepare must reserve topology state without committing generation",
            );
            assert_eq!(
                rendezvous.preflight_destination_topology_commit(sid, dst_lane),
                Ok(()),
                "destination prepare must stay pending until source commit closes it",
            );
        });
    }

    #[test]
    fn process_topology_intent_reports_occupied_destination_lane() {
        with_epf_test_rendezvous(|rendezvous| {
            let sid = SessionId::new(35);
            let occupying_sid = SessionId::new(36);
            let dst_lane = Lane::new(1);

            rendezvous
                .prepare_topology_control_scope(dst_lane)
                .expect("topology tests must bind topology storage");
            rendezvous.assoc.register(dst_lane, occupying_sid);

            let intent = TopologyIntent::new(
                RendezvousId::new(7),
                rendezvous.id,
                sid.raw(),
                Generation::new(5),
                Generation::new(6),
                3,
                7,
                Lane::new(0),
                dst_lane,
            );

            assert!(matches!(
                rendezvous.process_topology_intent(&intent),
                Err(TopologyError::LaneMismatch { expected, provided })
                    if expected == dst_lane && provided == dst_lane
            ));
        });
    }

    #[test]
    fn topology_ack_mismatch_reports_destination_fields_when_destination_mismatches() {
        let expected = TopologyAck::new(
            RendezvousId::new(1),
            RendezvousId::new(2),
            7,
            Generation::new(3),
            Lane::new(4),
            Lane::new(5),
            11,
            13,
        );

        let mut got = expected;
        got.dst_rv = RendezvousId::new(9);
        assert!(matches!(
            classify_topology_ack_mismatch(expected, got),
            TopologyError::RendezvousIdMismatch {
                expected,
                got
            } if expected == RendezvousId::new(2) && got == RendezvousId::new(9)
        ));

        let mut got = expected;
        got.new_lane = Lane::new(8);
        assert!(matches!(
            classify_topology_ack_mismatch(expected, got),
            TopologyError::LaneMismatch {
                expected,
                provided
            } if expected == Lane::new(5) && provided == Lane::new(8)
        ));

        let mut got = expected;
        got.new_gen = Generation::new(4);
        assert!(matches!(
            classify_topology_ack_mismatch(expected, got),
            TopologyError::StaleGeneration {
                lane,
                last,
                new
            } if lane == Lane::new(5)
                && last == Generation::new(3)
                && new == Generation::new(4)
        ));
    }

    #[test]
    fn state_restore_invalidates_post_snapshot_capability_authority() {
        with_epf_test_rendezvous(|rendezvous| {
            let sid = SessionId::new(23);
            let lane = Lane::new(1);
            let role = 5;
            let nonce = [0xA5; crate::control::cap::mint::CAP_NONCE_LEN];
            let handle = crate::control::cap::mint::EndpointHandle::new(sid, lane, role);
            let mut header = [0u8; crate::control::cap::mint::CAP_HEADER_LEN];

            rendezvous.assoc.register(lane, sid);
            rendezvous
                .r#gen
                .check_and_update(lane, Generation::ZERO)
                .expect("lane zero generation must initialize");
            rendezvous
                .r#gen
                .check_and_update(lane, Generation::new(1))
                .expect("generation must advance before snapshot");
            rendezvous
                .ensure_endpoint_resident_budget(EndpointResidentBudget::with_route_storage(
                    0, 0, 0, 1,
                ))
                .expect("capability restore test must bind cap storage");

            let snapshot = rendezvous.state_snapshot_at_lane(sid, lane);
            rendezvous
                .mint_cap::<crate::control::cap::mint::EndpointResource>(
                    sid,
                    lane,
                    crate::control::cap::mint::CapShot::One,
                    role,
                    nonce,
                    handle,
                )
                .expect("capability mint before snapshot must succeed");

            crate::control::cap::mint::CapHeader::new(
                sid,
                lane,
                role,
                crate::control::cap::mint::EndpointResource::TAG,
                0,
                ControlOp::Fence,
                crate::control::cap::mint::ControlPath::Local,
                crate::control::cap::mint::CapShot::One,
                crate::global::const_dsl::ControlScopeKind::None,
                0,
                0,
                0,
                crate::control::cap::mint::EndpointResource::encode_handle(&handle),
            )
            .encode(&mut header);
            let token = crate::control::cap::mint::GenericCapToken::<
                crate::control::cap::mint::EndpointResource,
            >::from_parts(
                nonce, header, [0; crate::control::cap::mint::CAP_TAG_LEN]
            );

            rendezvous
                .state_restore_at_lane(sid, lane, snapshot)
                .expect("restore must invalidate post-snapshot capability authority");

            assert!(
                matches!(rendezvous.claim_cap(&token), Err(CapError::UnknownToken)),
                "restore must not leave post-snapshot capability authority claimable",
            );
        });
    }

    #[test]
    fn state_restore_preserves_pre_snapshot_capability_authority() {
        with_epf_test_rendezvous(|rendezvous| {
            let sid = SessionId::new(24);
            let lane = Lane::new(1);
            let role = 6;
            let nonce = [0xB6; crate::control::cap::mint::CAP_NONCE_LEN];
            let handle = crate::control::cap::mint::EndpointHandle::new(sid, lane, role);
            let mut header = [0u8; crate::control::cap::mint::CAP_HEADER_LEN];

            rendezvous.assoc.register(lane, sid);
            rendezvous
                .r#gen
                .check_and_update(lane, Generation::ZERO)
                .expect("lane zero generation must initialize");
            rendezvous
                .r#gen
                .check_and_update(lane, Generation::new(1))
                .expect("generation must advance before capability mint");
            rendezvous
                .ensure_endpoint_resident_budget(EndpointResidentBudget::with_route_storage(
                    0, 0, 0, 1,
                ))
                .expect("capability restore test must bind cap storage");

            rendezvous
                .mint_cap::<crate::control::cap::mint::EndpointResource>(
                    sid,
                    lane,
                    crate::control::cap::mint::CapShot::One,
                    role,
                    nonce,
                    handle,
                )
                .expect("capability mint before snapshot must succeed");
            let snapshot = rendezvous.state_snapshot_at_lane(sid, lane);

            crate::control::cap::mint::CapHeader::new(
                sid,
                lane,
                role,
                crate::control::cap::mint::EndpointResource::TAG,
                0,
                ControlOp::Fence,
                crate::control::cap::mint::ControlPath::Local,
                crate::control::cap::mint::CapShot::One,
                crate::global::const_dsl::ControlScopeKind::None,
                0,
                0,
                0,
                crate::control::cap::mint::EndpointResource::encode_handle(&handle),
            )
            .encode(&mut header);
            let token = crate::control::cap::mint::GenericCapToken::<
                crate::control::cap::mint::EndpointResource,
            >::from_parts(
                nonce, header, [0; crate::control::cap::mint::CAP_TAG_LEN]
            );

            rendezvous
                .claim_cap(&token)
                .expect("pre-snapshot one-shot token must be claimable before restore");

            rendezvous
                .state_restore_at_lane(sid, lane, snapshot)
                .expect("restore must preserve snapshot-era capability authority");

            rendezvous
                .claim_cap(&token)
                .expect("restore must revive the snapshot-era one-shot capability state");
        });
    }

    #[test]
    fn state_restore_revives_pre_snapshot_release_authority() {
        with_epf_test_rendezvous(|rendezvous| {
            let sid = SessionId::new(31);
            let lane = Lane::new(1);
            let role = 7;
            let nonce = [0xC7; crate::control::cap::mint::CAP_NONCE_LEN];
            let handle = crate::control::cap::mint::EndpointHandle::new(sid, lane, role);
            let mut header = [0u8; crate::control::cap::mint::CAP_HEADER_LEN];

            rendezvous.assoc.register(lane, sid);
            rendezvous
                .r#gen
                .check_and_update(lane, Generation::ZERO)
                .expect("lane zero generation must initialize");
            rendezvous
                .r#gen
                .check_and_update(lane, Generation::new(1))
                .expect("generation must advance before capability mint");
            rendezvous
                .ensure_endpoint_resident_budget(EndpointResidentBudget::with_route_storage(
                    0, 0, 0, 1,
                ))
                .expect("release restore test must bind cap storage");

            rendezvous
                .mint_cap::<crate::control::cap::mint::EndpointResource>(
                    sid,
                    lane,
                    crate::control::cap::mint::CapShot::One,
                    role,
                    nonce,
                    handle,
                )
                .expect("capability mint before snapshot must succeed");
            let snapshot = rendezvous.state_snapshot_at_lane(sid, lane);

            crate::control::cap::mint::CapHeader::new(
                sid,
                lane,
                role,
                crate::control::cap::mint::EndpointResource::TAG,
                0,
                ControlOp::Fence,
                crate::control::cap::mint::ControlPath::Local,
                crate::control::cap::mint::CapShot::One,
                crate::global::const_dsl::ControlScopeKind::None,
                0,
                0,
                0,
                crate::control::cap::mint::EndpointResource::encode_handle(&handle),
            )
            .encode(&mut header);
            let token = crate::control::cap::mint::GenericCapToken::<
                crate::control::cap::mint::EndpointResource,
            >::from_parts(
                nonce, header, [0; crate::control::cap::mint::CAP_TAG_LEN]
            );

            assert_eq!(
                rendezvous.state_snapshots.available_cap_revision(lane),
                Some(1)
            );
            rendezvous.cap_release_ctx(lane).release(&nonce);

            assert!(
                matches!(
                    rendezvous.caps.claim_by_nonce(
                        &nonce,
                        sid,
                        lane,
                        crate::control::cap::mint::EndpointResource::TAG,
                        role,
                        crate::control::cap::mint::CapShot::One,
                        0,
                    ),
                    Err(CapError::UnknownToken)
                ),
                "release must hide authority in the capability table before restore",
            );
            assert!(
                matches!(rendezvous.claim_cap(&token), Err(CapError::UnknownToken)),
                "snapshot-aware release after snapshot must hide authority until restore",
            );

            rendezvous
                .state_restore_at_lane(sid, lane, snapshot)
                .expect("restore must revive pre-snapshot released authority");

            rendezvous
                .claim_cap(&token)
                .expect("restore must recreate pre-snapshot authority removed after snapshot");
        });
    }

    #[test]
    fn state_snapshot_finalization_rejects_restore_and_commit_replay() {
        with_epf_test_rendezvous(|rendezvous| {
            let sid = SessionId::new(11);
            let lane = Lane::new(1);

            rendezvous.assoc.register(lane, sid);
            rendezvous
                .r#gen
                .check_and_update(lane, Generation::ZERO)
                .expect("lane zero generation must initialize");

            let snapshot = rendezvous.state_snapshot_at_lane(sid, lane);
            rendezvous
                .state_restore_at_lane(sid, lane, snapshot)
                .expect("first restore must finalize the snapshot");

            assert!(matches!(
                rendezvous.state_restore_at_lane(sid, lane, snapshot),
                Err(StateRestoreError::AlreadyFinalized { sid: err_sid }) if err_sid == sid
            ));
            assert!(matches!(
                rendezvous.tx_commit_at_lane(sid, lane, snapshot),
                Err(TxCommitError::AlreadyFinalized { sid: err_sid }) if err_sid == sid
            ));
        });
    }

    #[test]
    fn topology_ack_emits_registered_tap_event() {
        with_epf_test_rendezvous(|rendezvous| {
            let sid = SessionId::new(19);
            let lane = Lane::new(1);
            rendezvous
                .prepare_topology_control_scope(lane)
                .expect("topology ack test must bind topology storage");
            let operands = TopologyOperands::new(
                RendezvousId::new(9),
                rendezvous.id,
                lane,
                lane,
                Generation::ZERO,
                Generation::new(2),
                31,
                37,
            );
            let envelope = CpCommand::topology_ack(sid, operands);

            assert_eq!(
                EffectRunner::run_effect(rendezvous, envelope),
                Err(CpError::Topology(
                    crate::control::cluster::error::TopologyError::InvalidState,
                )),
                "direct topology ack must fail closed because distributed topology ack is cluster-owned",
            );
            assert_eq!(
                rendezvous.preflight_destination_topology_commit(sid, lane),
                Err(TopologyError::NoPending { lane }),
                "rejected direct topology ack must not stage destination pending state",
            );

            rendezvous
                .acknowledge_topology_intent(&operands.intent(sid))
                .expect("cluster-owned topology ack helper must stage destination prepare");
            assert!(
                !rendezvous.is_session_registered(sid),
                "destination ack must stage the topology change without making the destination session live",
            );
            assert_eq!(
                rendezvous.preflight_destination_topology_commit(sid, lane),
                Ok(()),
                "ack must leave destination topology pending until the source commit finalizes it",
            );

            let mut cursor = 0usize;
            let events = rendezvous
                .tap()
                .events_since(&mut cursor, |event| {
                    (event.id == crate::observe::ids::TOPOLOGY_ACK).then_some(event)
                })
                .collect::<std::vec::Vec<_>>();

            assert_eq!(
                events.len(),
                1,
                "ack path must emit exactly one topology ack tap"
            );
            let event = events[0];
            let expected = ((operands.src_lane.as_wire() as u32) & 0xFF)
                | (((operands.dst_lane.as_wire() as u32) & 0xFF) << 8)
                | ((operands.new_gen.0 as u32) << 16);
            assert_eq!(event.arg0, expected);
            assert_eq!(event.arg1, sid.raw());
        });
    }

    #[test]
    fn abort_topology_state_clears_destination_prepare_explicitly() {
        with_epf_test_rendezvous(|rendezvous| {
            let sid = SessionId::new(34);
            let lane = Lane::new(1);

            rendezvous
                .prepare_topology_control_scope(lane)
                .expect("topology tests must bind topology storage");

            let intent = TopologyIntent::new(
                RendezvousId::new(7),
                rendezvous.id,
                sid.raw(),
                Generation::new(5),
                Generation::new(6),
                3,
                7,
                Lane::new(0),
                lane,
            );
            rendezvous
                .process_topology_intent(&intent)
                .expect("destination prepare must succeed before explicit abort");
            assert_eq!(
                rendezvous.lane_generation(lane),
                Generation::ZERO,
                "destination prepare must not advance generation before commit",
            );
            assert_eq!(
                rendezvous.preflight_destination_topology_commit(sid, lane),
                Ok(()),
                "destination prepare must be pending before explicit abort",
            );

            assert_eq!(
                rendezvous.abort_topology_state(sid),
                Ok(true),
                "explicit abort must clear destination-only prepared topology",
            );
            assert_eq!(
                rendezvous.preflight_destination_topology_commit(sid, lane),
                Err(TopologyError::NoPending { lane }),
                "explicit abort must remove destination pending topology state",
            );
            assert_eq!(
                rendezvous.r#gen.last(lane),
                None,
                "explicit abort must keep a fresh destination lane at its pre-ack generation state",
            );
        });
    }

    #[test]
    fn program_image_slot_exhaustion_does_not_leak_slab_frontier() {
        let summary = route_summary();
        let stamp = summary.stamp();
        with_image_test_rendezvous(|rendezvous| unsafe {
            let initial_frontier = rendezvous.image_frontier;
            let initial_floor = rendezvous.endpoint_lease_floor();

            assert!(
                rendezvous
                    .materialize_program_image_from_summary(stamp, &summary)
                    .is_none(),
                "slot exhaustion should reject materialization"
            );
            assert_eq!(
                rendezvous.image_frontier, initial_frontier,
                "program image slot exhaustion must not advance the slab frontier"
            );
            assert_eq!(
                rendezvous.endpoint_lease_floor(),
                initial_floor,
                "program image slot exhaustion must not shrink endpoint lease capacity"
            );

            assert!(
                rendezvous
                    .materialize_program_image_from_summary(stamp, &summary)
                    .is_none(),
                "repeated slot exhaustion should keep rejecting materialization"
            );
            assert_eq!(
                rendezvous.image_frontier, initial_frontier,
                "repeated failures must leave the slab frontier unchanged"
            );
            assert_eq!(
                rendezvous.endpoint_lease_floor(),
                initial_floor,
                "repeated failures must not change the endpoint lease floor"
            );
        });
    }

    #[test]
    fn role_image_slot_exhaustion_does_not_leak_slab_or_scratch_budget() {
        let summary = route_summary();
        let stamp = summary.stamp();
        with_image_test_rendezvous(|rendezvous| unsafe {
            let initial_frontier = rendezvous.image_frontier;
            let initial_scratch = rendezvous.scratch_reserved_bytes;
            let initial_floor = rendezvous.endpoint_lease_floor();

            assert!(
                rendezvous
                    .materialize_role_image_from_summary::<0>(stamp, &summary)
                    .is_none(),
                "slot exhaustion should reject role image materialization"
            );
            assert_eq!(
                rendezvous.image_frontier, initial_frontier,
                "role image slot exhaustion must not advance the slab frontier"
            );
            assert_eq!(
                rendezvous.scratch_reserved_bytes, initial_scratch,
                "role image slot exhaustion must not reserve scratch from an untracked image"
            );
            assert_eq!(
                rendezvous.endpoint_lease_floor(),
                initial_floor,
                "role image slot exhaustion must not shrink endpoint lease capacity"
            );

            assert!(
                rendezvous
                    .materialize_role_image_from_summary::<0>(stamp, &summary)
                    .is_none(),
                "repeated slot exhaustion should keep rejecting role image materialization"
            );
            assert_eq!(
                rendezvous.image_frontier, initial_frontier,
                "repeated role-image failures must leave the slab frontier unchanged"
            );
            assert_eq!(
                rendezvous.scratch_reserved_bytes, initial_scratch,
                "repeated role-image failures must not change scratch reservation"
            );
            assert_eq!(
                rendezvous.endpoint_lease_floor(),
                initial_floor,
                "repeated role-image failures must not change the endpoint lease floor"
            );
        });
    }

    #[test]
    fn route_table_capacity_stays_tied_to_lane_frame_depth() {
        with_image_test_rendezvous(|rendezvous| {
            rendezvous
                .ensure_endpoint_resident_budget(EndpointResidentBudget::with_route_storage(
                    2, 3, 0, 0,
                ))
                .expect("route resident budget should bind route storage");
            assert_eq!(
                rendezvous.routes.route_slots(),
                2,
                "route ledger lane-frame storage must stay tied to route depth"
            );
            assert_eq!(
                rendezvous.routes.lane_slots(),
                3,
                "route ledger lane storage must stay tied to the live lane span"
            );
        });
    }

    #[test]
    fn topology_table_binds_only_for_topology_control_scope() {
        with_image_test_rendezvous(|rendezvous| {
            assert!(!rendezvous.topology.is_bound());

            rendezvous.initialise_control_scope(Lane::new(0), ControlScopeKind::Loop);
            assert!(
                !rendezvous.topology.is_bound(),
                "non-topology control scopes must not bind topology storage"
            );

            rendezvous
                .prepare_topology_control_scope(Lane::new(0))
                .expect("topology control scope should bind topology storage");
            assert!(rendezvous.topology.is_bound());
        });
    }

    #[test]
    fn lane_lifecycle_clears_dynamic_policy_state() {
        with_epf_test_rendezvous(|rendezvous| {
            let lane = Lane::new(1);
            let sid = SessionId::new(29);
            let eff_index = EffIndex::new(11);
            let tag = 7;
            let policy = PolicyMode::dynamic(3);

            rendezvous
                .register_policy(lane, eff_index, tag, policy)
                .expect("dynamic policy registration must bind policy storage");
            assert_eq!(rendezvous.policy(lane, eff_index, tag), Some(policy));

            rendezvous
                .activate_lane_for_test(sid, lane)
                .expect("first attach must clear stale policy state before opening the lane");
            assert_eq!(
                rendezvous.policy(lane, eff_index, tag),
                None,
                "first attach must clear stale lane policy state",
            );

            rendezvous
                .register_policy(lane, eff_index, tag, policy)
                .expect("policy state should remain writable after attach");
            assert_eq!(rendezvous.release_lane(lane), Some(sid));
            assert_eq!(
                rendezvous.policy(lane, eff_index, tag),
                None,
                "lane release must own dynamic policy cleanup",
            );
        });
    }

    #[test]
    fn state_restore_preserves_live_session_policy_image() {
        with_epf_test_rendezvous(|rendezvous| {
            let lane = Lane::new(1);
            let sid = SessionId::new(30);
            let eff_index = EffIndex::new(12);
            let tag = 9;
            let policy = PolicyMode::dynamic(7);

            rendezvous.assoc.register(lane, sid);
            rendezvous
                .r#gen
                .check_and_update(lane, Generation::ZERO)
                .expect("lane zero generation must initialize");
            rendezvous
                .r#gen
                .check_and_update(lane, Generation::new(1))
                .expect("generation must advance before snapshot");
            rendezvous
                .register_policy(lane, eff_index, tag, policy)
                .expect("policy image should be writable before snapshot");

            let snapshot = rendezvous.state_snapshot_at_lane(sid, lane);
            rendezvous
                .state_restore_at_lane(sid, lane, snapshot)
                .expect("restore should not clear the live session policy image");

            assert_eq!(
                rendezvous.policy(lane, eff_index, tag),
                Some(policy),
                "restore must preserve the session policy image for the live lane",
            );
        });
    }

    #[test]
    #[should_panic(expected = "capability nonce counter exhausted")]
    fn next_nonce_seed_panics_on_overflow() {
        with_epf_test_rendezvous(|rendezvous| {
            rendezvous.cap_nonce.set(u64::MAX);
            let _ = rendezvous.next_nonce_seed();
        });
    }

    #[test]
    fn trim_resident_headers_reclaims_frontier_when_no_images_remain_above_sidecars() {
        with_image_test_rendezvous(|rendezvous| {
            let initial_frontier = rendezvous.image_frontier;
            rendezvous
                .ensure_endpoint_resident_budget(EndpointResidentBudget::with_route_storage(
                    2, 3, 3, 8,
                ))
                .expect("resident sidecars should bind");
            assert!(
                rendezvous.image_frontier > initial_frontier,
                "resident sidecars must consume persistent bytes before trimming"
            );

            rendezvous.trim_resident_headers_to_live_budget();

            assert_eq!(
                rendezvous.image_frontier, initial_frontier,
                "trimming empty resident headers must return the frontier when nothing remains above them"
            );
            assert_eq!(rendezvous.routes.route_slots(), 0);
            assert_eq!(rendezvous.loops.loop_slots(), 0);
            assert_eq!(rendezvous.caps.capacity(), 0);
        });
    }

    #[test]
    fn resident_sidecars_reuse_freed_regions_before_growing_frontier_again() {
        let summary = route_summary();
        with_image_test_rendezvous_slots(1, |rendezvous| unsafe {
            rendezvous
                .ensure_endpoint_resident_budget(EndpointResidentBudget::with_route_storage(
                    2, 3, 3, 8,
                ))
                .expect("resident sidecars should bind");
            let frontier_after_sidecars = rendezvous.image_frontier;

            rendezvous
                .materialize_program_image_from_summary(summary.stamp(), &summary)
                .expect("program image should materialize above the sidecar section");
            let frontier_after_image = rendezvous.image_frontier;
            assert!(
                frontier_after_image > frontier_after_sidecars,
                "program image must sit above the resident sidecars to validate reuse"
            );

            rendezvous.trim_resident_headers_to_live_budget();
            assert_eq!(
                rendezvous.image_frontier, frontier_after_image,
                "freeing sidecars below a live image should not move the frontier immediately"
            );

            rendezvous
                .ensure_endpoint_resident_budget(EndpointResidentBudget::with_route_storage(
                    2, 3, 3, 8,
                ))
                .expect("resident sidecars should rebind from freed regions");
            assert_eq!(
                rendezvous.image_frontier, frontier_after_image,
                "rebinding resident sidecars must reuse freed regions instead of growing the frontier"
            );
        });
    }

    #[test]
    fn external_sidecar_free_reclaims_frontier_alignment_padding() {
        with_image_test_rendezvous(|rendezvous| {
            let initial_frontier = rendezvous.image_frontier;
            let align = core::mem::align_of::<u128>();
            let head_bytes = if (initial_frontier as usize + 1) % align == 0 {
                2
            } else {
                1
            };

            let (head_ptr, head_reclaim_delta) = rendezvous
                .allocate_external_persistent_sidecar_bytes(head_bytes, 1)
                .expect("unaligned external sidecar should bind");
            let frontier_after_head = rendezvous.image_frontier;

            let (aligned_ptr, aligned_reclaim_delta) = rendezvous
                .allocate_external_persistent_sidecar_bytes(8, align)
                .expect("aligned external sidecar should bind");
            assert!(
                aligned_reclaim_delta > 0,
                "aligned external sidecar must record reclaimed prefix padding when frontier is unaligned"
            );

            rendezvous.free_external_persistent_sidecar_bytes(
                aligned_ptr,
                8,
                aligned_reclaim_delta,
            );
            assert_eq!(
                rendezvous.image_frontier, frontier_after_head,
                "freeing the top external sidecar must reclaim its alignment padding back to the previous frontier"
            );

            rendezvous.free_external_persistent_sidecar_bytes(
                head_ptr,
                head_bytes,
                head_reclaim_delta,
            );
            assert_eq!(
                rendezvous.image_frontier, initial_frontier,
                "freeing all external sidecars must return the frontier to its starting point"
            );
        });
    }

    #[test]
    fn program_image_slot_reuses_unpinned_storage_without_frontier_growth() {
        let summary_a = route_summary();
        let summary_b = route_summary_alt();
        with_image_test_rendezvous_slots(1, |rendezvous| unsafe {
            let image_a = rendezvous
                .materialize_program_image_from_summary(summary_a.stamp(), &summary_a)
                .expect("materialize first program image");
            let first_frontier = rendezvous.image_frontier;
            let program_slot = rendezvous
                .pin_program_image(summary_a.stamp())
                .expect("pin first program image");
            rendezvous.unpin_program_image_slot(program_slot as usize);

            let image_b = rendezvous
                .materialize_program_image_from_summary(summary_b.stamp(), &summary_b)
                .expect("reuse program image slot");
            assert_eq!(
                rendezvous.image_frontier, first_frontier,
                "reusing an unpinned program image slot must not advance the persistent frontier"
            );
            assert_eq!(
                image_a as usize, image_b as usize,
                "reused program image slot must keep using the same storage when the replacement fits"
            );
        });
    }

    #[test]
    fn releasing_endpoint_lease_unpins_compiled_images_for_reuse() {
        let summary_a = route_summary();
        let summary_b = route_summary_alt();
        with_image_test_rendezvous_slots(1, |rendezvous| unsafe {
            let image_a = rendezvous
                .materialize_program_image_from_summary(summary_a.stamp(), &summary_a)
                .expect("materialize first program image");
            rendezvous
                .materialize_role_image_from_summary::<0>(summary_a.stamp(), &summary_a)
                .expect("materialize first role image");
            let (lease_slot, generation, _, _) = rendezvous
                .allocate_endpoint_lease(7, 1, EndpointResidentBudget::ZERO)
                .expect("lease endpoint slot");
            assert!(
                rendezvous.pin_endpoint_images::<0>(lease_slot, generation, summary_a.stamp()),
                "active endpoint must pin compiled images"
            );
            let frontier_after_first = rendezvous.image_frontier;

            assert!(
                rendezvous
                    .materialize_program_image_from_summary(summary_b.stamp(), &summary_b)
                    .is_none(),
                "pinned image slot must not be reusable while the endpoint is live"
            );
            rendezvous.release_endpoint_lease(lease_slot, generation);

            let image_b = rendezvous
                .materialize_program_image_from_summary(summary_b.stamp(), &summary_b)
                .expect("released endpoint must make image slot reusable");
            assert_eq!(
                rendezvous.image_frontier, frontier_after_first,
                "released endpoint must let the image bank reuse in-place storage"
            );
            assert_eq!(
                image_a as usize, image_b as usize,
                "released endpoint must hand the same program-image storage back to the bank"
            );
        });
    }

    #[test]
    fn public_path_image_bank_is_wider_than_endpoint_lease_capacity() {
        let summary_a = route_summary();
        let summary_b = route_summary_alt();
        with_image_test_rendezvous_public_slots(1, |rendezvous| unsafe {
            assert_eq!(
                rendezvous.endpoint_lease_capacity,
                EndpointLeaseId::from(1u8),
                "public-path lease budget should keep the requested endpoint slot count"
            );
            assert!(
                EndpointLeaseId::from(rendezvous.image_slot_capacity)
                    > rendezvous.endpoint_lease_capacity,
                "public-path image bank must decouple compiled image capacity from endpoint leases"
            );

            rendezvous
                .materialize_program_image_from_summary(summary_a.stamp(), &summary_a)
                .expect("materialize first program image");
            let pinned = rendezvous
                .pin_program_image(summary_a.stamp())
                .expect("pin first program image");

            assert!(
                rendezvous
                    .materialize_program_image_from_summary(summary_b.stamp(), &summary_b)
                    .is_some(),
                "a pinned program image must not block a second compiled image on the public path"
            );

            rendezvous.unpin_program_image_slot(pinned as usize);
        });
    }
}

// ============================================================================
// Facet API - ZST-based constrained access
// ============================================================================

impl<'rv, 'cfg, T, U, C, E> Rendezvous<'rv, 'cfg, T, U, C, E>
where
    'cfg: 'rv,
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
    /// Borrow topology coordination state as a constrained facet.
    pub(crate) fn topology_facet(&mut self) -> TopologyFacet<T, U, C, E> {
        TopologyFacet::new()
    }

    /// Borrow observation ring as a constrained facet.
    pub(crate) fn observe_facet(&self) -> ObserveFacet<'_, 'cfg> {
        ObserveFacet::new(self.tap())
    }
}

/// Topology-focused facet that exposes only topology coordination operations.
#[derive(Default)]
pub(crate) struct TopologyFacet<T, U, C, E>(PhantomData<(T, U, C, E)>)
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable;

impl<T, U, C, E> Copy for TopologyFacet<T, U, C, E>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
}

impl<T, U, C, E> Clone for TopologyFacet<T, U, C, E>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
    fn clone(&self) -> Self {
        *self
    }
}

impl<T, U, C, E> TopologyFacet<T, U, C, E>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
    #[inline]
    pub(crate) const fn new() -> Self {
        Self(PhantomData)
    }

    pub(crate) fn begin_from_intent(
        self,
        rendezvous: &mut Rendezvous<'_, '_, T, U, C, E>,
        intent: TopologyIntent,
    ) -> Result<(), super::error::TopologyError> {
        rendezvous.topology_begin_from_intent(intent)
    }
}

/// Observation facet that exposes tap emission without leaking rendezvous state.
#[derive(Clone, Copy)]
pub(crate) struct ObserveFacet<'tap, 'cfg> {
    tap: &'tap crate::observe::core::TapRing<'cfg>,
}

impl<'tap, 'cfg> ObserveFacet<'tap, 'cfg> {
    #[inline]
    pub(crate) const fn new(tap: &'tap crate::observe::core::TapRing<'cfg>) -> Self {
        Self { tap }
    }

    /// Borrow the underlying tap ring (read-only).
    #[inline]
    pub(crate) fn tap(&self) -> &'tap crate::observe::core::TapRing<'cfg> {
        self.tap
    }
}
