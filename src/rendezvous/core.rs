//! Rendezvous (control plane) primitives.
//!
//! The rendezvous component owns the association tables that map session
//! identifiers to transport lanes. A fully-fledged implementation would manage
//! splice/delegate bookkeeping and generation counters; the current version
//! keeps just enough structure to support endpoint scaffolding while leaving
//! clear extension points.

use core::{cell::Cell, marker::PhantomData, ops::Range};

use super::{
    association::AssocTable,
    capability::{CapEntry, CapTable},
    error::{
        CancelError, CapError, CheckpointError, CommitError, GenError, GenerationRecord,
        RendezvousError, RollbackError, SpliceError,
    },
    port::Port,
    slots::SlotArena,
    splice::{DistributedSpliceTable, PendingSplice, SpliceStateTable},
    tables::{CheckpointTable, GenTable, LoopTable, PolicyTable, RouteTable, VmCapsTable},
};
#[cfg(test)]
use crate::runtime::consts::LANES_MAX;
use crate::{
    control::{
        automaton::txn::{NoopTap, Txn},
        brand::{self, Guard},
        cap::mint::{
            CapShot, CapsMask, EndpointHandle, EndpointResource, GenericCapToken, NonceSeed,
            ResourceKind, VerifiedCap,
        },
        cluster::{
            core::{CpCommand, EffectRunner, SpliceOperands},
            effects::CpEffect,
            error::CpError,
        },
        types::{IncreasingGen, One},
    },
    eff::EffIndex,
    endpoint::affine::LaneGuard,
    epf::host::HostSlots,
    global::compiled::{CompiledProgramImage, CompiledRoleImage, LoweringSummary, ProgramStamp},
    global::const_dsl::{ControlScopeKind, PolicyMode},
    global::typestate::RoleCompileScratch,
    observe::core::{TapEvent, TapRing, emit},
    observe::{
        events::{DelegBegin, DelegSplice, LaneRelease, RawEvent, RollbackOk},
        ids, policy_abort, policy_trap,
    },
    runtime::config::{Clock, Config, ConfigParts, CounterClock},
    runtime::consts::{DefaultLabelUniverse, LabelUniverse},
    transport::{Transport, TransportEventKind, TransportMetrics},
};

const ENDPOINT_TAG: u8 = 0;
use super::splice::LocalSpliceInvariant;
use crate::control::automaton::distributed::{SpliceAck, SpliceIntent};
use crate::control::types::{Generation, Lane, RendezvousId, SessionId};

#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct EndpointLeaseId(u16);

impl EndpointLeaseId {
    pub(crate) const ZERO: Self = Self(0);
    pub(crate) const MAX: Self = Self(u16::MAX);
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
struct ProgramImageSlot {
    stamp: ProgramStamp,
    offset: u32,
    len: u32,
    pins: u16,
    occupied: bool,
}

impl ProgramImageSlot {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EndpointLeaseSlot {
    pub(crate) generation: u32,
    pub(crate) offset: u32,
    pub(crate) len: u32,
    pub(crate) resident_budget: EndpointResidentBudget,
    pub(crate) program_image_slot: u8,
    pub(crate) role_image_slot: u8,
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
    program_images: *mut ProgramImageSlot,
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
    checkpoints: CheckpointTable,
    splice: SpliceStateTable,
    distributed_splice: DistributedSpliceTable,
    cap_nonce: Cell<u32>,
    caps: CapTable,
    loops: LoopTable,
    routes: RouteTable,
    policies: PolicyTable,
    vm_caps: VmCapsTable,
    slot_arena: SlotArena,
    host_slots: HostSlots<'cfg>,
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
    delegate: Option<DelegateContext>,
}

impl EffectContext {
    fn new(sid: SessionId, lane: Lane) -> Self {
        Self {
            sid,
            lane,
            generation: None,
            fences: None,
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
    Rollback(RollbackError),
    Commit(super::error::CommitError),
    MissingGeneration,
    Unsupported,
    Splice(SpliceError),
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
            base + self.image_frontier as usize,
            CompiledProgramImage::persistent_align(),
        )
        .saturating_sub(base);
        start + CompiledProgramImage::max_persistent_bytes() - self.image_frontier as usize
    }

    #[inline(always)]
    pub(crate) fn role_image_guard_bytes(&self, bytes: usize) -> usize {
        let (slab_ptr, _) = self.slab_ptr_and_len();
        let base = slab_ptr as usize;
        let start = Self::align_up(
            base + self.image_frontier as usize,
            CompiledRoleImage::persistent_align(),
        )
        .saturating_sub(base);
        start + bytes - self.image_frontier as usize
    }

    #[inline(always)]
    pub(crate) fn program_and_role_image_guard_bytes(&self, role_image_bytes: usize) -> usize {
        let (slab_ptr, _) = self.slab_ptr_and_len();
        let base = slab_ptr as usize;
        let program_end = Self::align_up(
            base + self.image_frontier as usize,
            CompiledProgramImage::persistent_align(),
        ) + CompiledProgramImage::max_persistent_bytes();
        let role_end =
            Self::align_up(program_end, CompiledRoleImage::persistent_align()) + role_image_bytes;
        role_end - self.image_frontier as usize - base
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
        let start = self.image_frontier as usize;
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
        slot: &ProgramImageSlot,
    ) -> *const CompiledProgramImage {
        let (slab_ptr, _) = self.slab_ptr_and_len();
        unsafe {
            slab_ptr
                .add(slot.offset as usize)
                .cast::<CompiledProgramImage>()
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

    #[cfg(test)]
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
                    required_lane_slots,
                    reclaim_delta,
                );
            }
        } else {
            unsafe {
                self.routes.migrate_from_storage(
                    storage,
                    required_frame_slots,
                    required_lane_slots,
                );
                self.routes.rebind_from_storage(
                    storage,
                    required_frame_slots,
                    required_lane_slots,
                    reclaim_delta,
                );
            }
            self.free_bound_persistent_region(old_reclaim_offset, old_ptr, old_bytes);
        }
        Some(())
    }

    fn ensure_loop_table_capacity(&mut self, required_slots: usize) -> Option<()> {
        if required_slots == 0 || self.loops.loop_slots() >= required_slots {
            return Some(());
        }
        let prior_frontier = self.image_frontier;
        let (storage, storage_offset) = unsafe {
            self.allocate_persistent_sidecar_bytes(
                LoopTable::storage_bytes(required_slots),
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
                self.loops
                    .bind_from_storage(storage, required_slots, reclaim_delta);
            }
        } else {
            unsafe {
                self.loops.migrate_from_storage(storage, required_slots);
                self.loops
                    .rebind_from_storage(storage, required_slots, reclaim_delta);
            }
            self.free_bound_persistent_region(old_reclaim_offset, old_ptr, old_bytes);
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

    fn ensure_splice_table_storage(&mut self) -> Option<()> {
        if self.splice.is_bound() {
            return Some(());
        }
        let (storage, _) = unsafe {
            self.allocate_persistent_sidecar_bytes(
                SpliceStateTable::storage_bytes(),
                SpliceStateTable::storage_align(),
            )
        }?;
        unsafe {
            self.splice.bind_from_storage(storage);
        }
        Some(())
    }

    fn ensure_distributed_splice_storage(&mut self) -> Option<()> {
        if self.distributed_splice.is_bound() {
            return Some(());
        }
        let (storage, _) = unsafe {
            self.allocate_persistent_sidecar_bytes(
                DistributedSpliceTable::storage_bytes(),
                DistributedSpliceTable::storage_align(),
            )
        }?;
        unsafe {
            self.distributed_splice.bind_from_storage(storage);
        }
        Some(())
    }

    pub(crate) fn ensure_splice_control_storage(&mut self) -> Option<()> {
        self.ensure_splice_table_storage()?;
        self.ensure_distributed_splice_storage()?;
        Some(())
    }

    pub(crate) fn prepare_splice_control_scope(&mut self, lane: Lane) -> Option<()> {
        self.ensure_splice_control_storage()?;
        self.initialise_control_scope(lane, ControlScopeKind::Splice);
        Some(())
    }

    fn ensure_policy_table_storage(&mut self) -> Option<()> {
        if self.policies.is_bound() {
            return Some(());
        }
        let (storage, _) = unsafe {
            self.allocate_persistent_sidecar_bytes(
                PolicyTable::storage_bytes(),
                PolicyTable::storage_align(),
            )
        }?;
        unsafe {
            self.policies.bind_from_storage(storage);
        }
        Some(())
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

    #[cfg(test)]
    #[inline]
    pub(crate) fn record_endpoint_resident_budget(
        &mut self,
        lease_slot: EndpointLeaseId,
        generation: u32,
        budget: EndpointResidentBudget,
    ) {
        if let Some(slot) = self.endpoint_lease_mut(lease_slot, generation) {
            slot.resident_budget = budget;
        }
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
    fn role_image_slot_index<const ROLE: u8>(&self, stamp: ProgramStamp) -> Option<usize> {
        let mut idx = 0usize;
        while idx < self.image_slot_capacity as usize {
            let slot = unsafe { &*self.role_images.add(idx) };
            if slot.occupied && slot.stamp == stamp && slot.role == ROLE {
                return Some(idx);
            }
            idx += 1;
        }
        None
    }

    #[inline]
    fn pin_role_image<const ROLE: u8>(&mut self, stamp: ProgramStamp) -> Option<u8> {
        let idx = self.role_image_slot_index::<ROLE>(stamp)?;
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
        let Some(role_image_slot) = self.pin_role_image::<ROLE>(stamp) else {
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

    #[inline(never)]
    pub(crate) unsafe fn materialize_program_image_from_summary(
        &mut self,
        stamp: ProgramStamp,
        summary: &LoweringSummary,
    ) -> Option<*const CompiledProgramImage> {
        if let Some(idx) = self.program_image_slot_index(stamp) {
            let slot = unsafe { &*self.program_images.add(idx) };
            return Some(unsafe { self.program_image_from_slot(slot) });
        }
        let Some(insert_idx) = self.first_free_program_image_slot() else {
            return unsafe { self.recycle_program_image_from_summary(stamp, summary) };
        };
        let counts = CompiledProgramImage::counts(summary);
        let bytes = CompiledProgramImage::persistent_bytes_for_counts(counts);
        let (ptr, offset) = unsafe {
            self.allocate_persistent_image_bytes(bytes, CompiledProgramImage::persistent_align())
        }?;
        unsafe {
            crate::global::compiled::init_compiled_program_image_from_summary(
                ptr.cast::<CompiledProgramImage>(),
                summary,
            );
        }
        let slot = unsafe { &mut *self.program_images.add(insert_idx) };
        *slot = ProgramImageSlot {
            stamp,
            offset,
            len: bytes as u32,
            pins: 0,
            occupied: true,
        };
        Some(ptr.cast::<CompiledProgramImage>())
    }

    #[cold]
    #[inline(never)]
    unsafe fn recycle_program_image_from_summary(
        &mut self,
        stamp: ProgramStamp,
        summary: &LoweringSummary,
    ) -> Option<*const CompiledProgramImage> {
        let counts = CompiledProgramImage::counts(summary);
        let bytes = CompiledProgramImage::persistent_bytes_for_counts(counts);
        if let Some(insert_idx) = self.first_reusable_program_image_slot(bytes) {
            let slot = unsafe { &mut *self.program_images.add(insert_idx) };
            let ptr = unsafe {
                self.slab_ptr_and_len()
                    .0
                    .add(slot.offset as usize)
                    .cast::<CompiledProgramImage>()
            };
            unsafe {
                crate::global::compiled::init_compiled_program_image_from_summary(ptr, summary);
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
                        .cast::<CompiledProgramImage>()
                };
                (ptr.cast::<u8>(), offset, slot.len, None)
            } else {
                let (ptr, offset) = unsafe {
                    self.allocate_persistent_image_bytes(
                        bytes,
                        CompiledProgramImage::persistent_align(),
                    )
                }?;
                (ptr, offset, bytes as u32, Some((slot.offset, slot.len)))
            }
        };
        unsafe {
            crate::global::compiled::init_compiled_program_image_from_summary(
                ptr.cast::<CompiledProgramImage>(),
                summary,
            );
        }
        if let Some((old_offset, old_len)) = released_region {
            self.release_persistent_region(old_offset, old_len);
        }
        let slot = unsafe { &mut *self.program_images.add(insert_idx) };
        *slot = ProgramImageSlot {
            stamp,
            offset,
            len: reserved_len,
            pins: 0,
            occupied: true,
        };
        Some(ptr.cast::<CompiledProgramImage>())
    }

    #[inline]
    pub(crate) fn has_program_image(&self, stamp: ProgramStamp) -> bool {
        self.program_image_slot_index(stamp).is_some()
    }

    #[inline]
    pub(crate) fn program_image(&self, stamp: ProgramStamp) -> Option<*const CompiledProgramImage> {
        let idx = self.program_image_slot_index(stamp)?;
        let slot = unsafe { &*self.program_images.add(idx) };
        Some(unsafe { self.program_image_from_slot(slot) })
    }

    #[inline(never)]
    unsafe fn pinned_role_image_from_slot<const ROLE: u8>(
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
        let _ = ROLE;
        role_ptr
    }

    #[cfg(test)]
    #[inline(never)]
    unsafe fn materialize_new_role_image_from_summary<const ROLE: u8>(
        &mut self,
        insert_idx: usize,
        stamp: ProgramStamp,
        summary: &LoweringSummary,
        scratch: &mut RoleCompileScratch,
    ) -> Option<*const CompiledRoleImage> {
        let scope_count = summary.stamp().scope_count();
        let eff_count = summary.view().as_slice().len();
        let bytes =
            CompiledRoleImage::persistent_bytes_for_counts(scope_count, scope_count, eff_count);
        let (ptr, offset) = unsafe {
            self.allocate_persistent_image_bytes(bytes, CompiledRoleImage::persistent_align())
        }?;
        unsafe {
            CompiledRoleImage::init_from_summary::<ROLE>(
                ptr.cast::<CompiledRoleImage>(),
                summary,
                scratch,
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
            role: ROLE,
            offset,
            len: bytes as u32,
            pins: 0,
            occupied: true,
        };
        Some(ptr.cast::<CompiledRoleImage>())
    }

    #[inline(never)]
    unsafe fn materialize_new_role_image_from_summary_for_program<const ROLE: u8>(
        &mut self,
        insert_idx: usize,
        stamp: ProgramStamp,
        summary: &LoweringSummary,
        scratch: &mut RoleCompileScratch,
        layout: crate::global::role_program::RoleImageLayoutInput,
    ) -> Option<*const CompiledRoleImage> {
        let bytes = CompiledRoleImage::persistent_bytes_for_program(layout);
        let (ptr, offset) = unsafe {
            self.allocate_persistent_image_bytes(bytes, CompiledRoleImage::persistent_align())
        }?;
        unsafe {
            crate::global::compiled::init_compiled_role_image_from_summary::<ROLE>(
                ptr.cast::<CompiledRoleImage>(),
                summary,
                scratch,
                layout,
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
            role: ROLE,
            offset,
            len: bytes as u32,
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
        scratch: &mut RoleCompileScratch,
    ) -> Option<*const CompiledRoleImage> {
        if let Some(idx) = self.role_image_slot_index::<ROLE>(stamp) {
            let slot = unsafe { &*self.role_images.add(idx) };
            return Some(unsafe { self.pinned_role_image_from_slot::<ROLE>(slot) });
        }
        let Some(insert_idx) = self.first_free_role_image_slot() else {
            return unsafe {
                self.recycle_role_image_from_summary::<ROLE>(stamp, summary, scratch)
            };
        };
        unsafe {
            self.materialize_new_role_image_from_summary::<ROLE>(
                insert_idx, stamp, summary, scratch,
            )
        }
    }

    #[inline(never)]
    pub(crate) unsafe fn materialize_role_image_from_summary_for_program<const ROLE: u8>(
        &mut self,
        stamp: ProgramStamp,
        summary: &LoweringSummary,
        scratch: &mut RoleCompileScratch,
        layout: crate::global::role_program::RoleImageLayoutInput,
    ) -> Option<*const CompiledRoleImage> {
        if let Some(idx) = self.role_image_slot_index::<ROLE>(stamp) {
            let slot = unsafe { &*self.role_images.add(idx) };
            return Some(unsafe { self.pinned_role_image_from_slot::<ROLE>(slot) });
        }
        let Some(insert_idx) = self.first_free_role_image_slot() else {
            return unsafe {
                self.recycle_role_image_from_summary_for_program::<ROLE>(
                    stamp, summary, scratch, layout,
                )
            };
        };
        unsafe {
            self.materialize_new_role_image_from_summary_for_program::<ROLE>(
                insert_idx, stamp, summary, scratch, layout,
            )
        }
    }

    #[cfg(test)]
    #[cold]
    #[inline(never)]
    unsafe fn recycle_role_image_from_summary<const ROLE: u8>(
        &mut self,
        stamp: ProgramStamp,
        summary: &LoweringSummary,
        scratch: &mut RoleCompileScratch,
    ) -> Option<*const CompiledRoleImage> {
        let scope_count = summary.stamp().scope_count();
        let eff_count = summary.view().as_slice().len();
        let bytes =
            CompiledRoleImage::persistent_bytes_for_counts(scope_count, scope_count, eff_count);
        if let Some(insert_idx) = self.first_reusable_role_image_slot(bytes) {
            let slot = unsafe { &mut *self.role_images.add(insert_idx) };
            let ptr = unsafe {
                self.slab_ptr_and_len()
                    .0
                    .add(slot.offset as usize)
                    .cast::<CompiledRoleImage>()
            };
            unsafe {
                CompiledRoleImage::init_from_summary::<ROLE>(ptr, summary, scratch);
            }
            let reserved =
                Self::frontier_scratch_guard_bytes(unsafe { (*ptr).frontier_scratch_layout() })
                    as u32;
            self.reserve_scratch_reserved_bytes(reserved);
            slot.stamp = stamp;
            slot.role = ROLE;
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
            CompiledRoleImage::init_from_summary::<ROLE>(
                ptr.cast::<CompiledRoleImage>(),
                summary,
                scratch,
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
            role: ROLE,
            offset,
            len: reserved_len,
            pins: 0,
            occupied: true,
        };
        Some(ptr.cast::<CompiledRoleImage>())
    }

    #[cold]
    #[inline(never)]
    unsafe fn recycle_role_image_from_summary_for_program<const ROLE: u8>(
        &mut self,
        stamp: ProgramStamp,
        summary: &LoweringSummary,
        scratch: &mut RoleCompileScratch,
        layout: crate::global::role_program::RoleImageLayoutInput,
    ) -> Option<*const CompiledRoleImage> {
        let bytes = CompiledRoleImage::persistent_bytes_for_program(layout);
        if let Some(insert_idx) = self.first_reusable_role_image_slot(bytes) {
            let slot = unsafe { &mut *self.role_images.add(insert_idx) };
            let ptr = unsafe {
                self.slab_ptr_and_len()
                    .0
                    .add(slot.offset as usize)
                    .cast::<CompiledRoleImage>()
            };
            unsafe {
                crate::global::compiled::init_compiled_role_image_from_summary::<ROLE>(
                    ptr, summary, scratch, layout,
                );
            }
            let reserved =
                Self::frontier_scratch_guard_bytes(unsafe { (*ptr).frontier_scratch_layout() })
                    as u32;
            self.reserve_scratch_reserved_bytes(reserved);
            slot.stamp = stamp;
            slot.role = ROLE;
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
            crate::global::compiled::init_compiled_role_image_from_summary::<ROLE>(
                ptr.cast::<CompiledRoleImage>(),
                summary,
                scratch,
                layout,
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
            role: ROLE,
            offset,
            len: reserved_len,
            pins: 0,
            occupied: true,
        };
        Some(ptr.cast::<CompiledRoleImage>())
    }

    #[inline]
    pub(crate) fn has_role_image<const ROLE: u8>(&self, stamp: ProgramStamp) -> bool {
        self.role_image_slot_index::<ROLE>(stamp).is_some()
    }

    #[inline]
    pub(crate) fn role_image<const ROLE: u8>(
        &self,
        stamp: ProgramStamp,
    ) -> Option<*const CompiledRoleImage> {
        let (slab_ptr, _) = self.slab_ptr_and_len();
        let idx = self.role_image_slot_index::<ROLE>(stamp)?;
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

    #[cfg(test)]
    #[inline]
    pub(crate) fn reserve_endpoint_lease(
        &mut self,
        resident_budget: EndpointResidentBudget,
    ) -> Option<(EndpointLeaseId, u32)> {
        unsafe {
            self.allocate_endpoint_lease(1, 1, resident_budget)
                .map(|(slot, generation, _, _)| (slot, generation))
        }
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

    #[inline]
    pub(crate) fn shorten<'short>(&'short self) -> &'short Rendezvous<'short, 'cfg, T, U, C, E>
    where
        'cfg: 'short,
    {
        let ptr: *const Self = self;
        unsafe { &*ptr.cast::<Rendezvous<'short, 'cfg, T, U, C, E>>() }
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
    fn policy_digest(&self, slot: crate::epf::vm::Slot) -> u32 {
        #[cfg(test)]
        {
            self.host_slots.active_digest(slot)
        }
        #[cfg(not(test))]
        {
            let _ = slot;
            0
        }
    }

    #[inline]
    fn policy_mode(&self, slot: crate::epf::vm::Slot) -> crate::epf::PolicyMode {
        #[cfg(test)]
        {
            self.host_slots.policy_mode(slot)
        }
        #[cfg(not(test))]
        {
            let _ = slot;
            crate::epf::PolicyMode::Enforce
        }
    }

    #[inline]
    fn last_policy_fuel_used(&self, slot: crate::epf::vm::Slot) -> u16 {
        #[cfg(test)]
        {
            self.host_slots.last_fuel_used(slot)
        }
        #[cfg(not(test))]
        {
            let _ = slot;
            0
        }
    }

    #[inline]
    fn run_policy<F>(
        &self,
        slot: crate::epf::vm::Slot,
        event: &crate::observe::core::TapEvent,
        caps: CapsMask,
        session: Option<SessionId>,
        lane: Option<Lane>,
        configure: F,
    ) -> crate::epf::Action
    where
        F: FnOnce(&mut crate::epf::vm::VmCtx<'_>),
    {
        #[cfg(test)]
        {
            return crate::epf::run_with(
                &self.host_slots,
                slot,
                event,
                caps,
                session,
                lane,
                configure,
            );
        }
        #[cfg(not(test))]
        {
            let mut ctx = crate::epf::vm::VmCtx::new(slot, event, caps);
            if let Some(session) = session {
                ctx.set_session(session);
            }
            if let Some(lane) = lane {
                ctx.set_lane(lane);
            }
            configure(&mut ctx);
            crate::epf::Action::Proceed
        }
    }

    fn prepare_distributed_splice_operands(
        &self,
        sid: SessionId,
        src_lane: Lane,
        dst_rv: RendezvousId,
        dst_lane: Lane,
        fences: Option<(u32, u32)>,
    ) -> Result<SpliceOperands, CpError> {
        let intent = self
            .begin_distributed_splice(sid, src_lane, dst_rv, dst_lane, fences)
            .map_err(map_splice_error)?;
        Ok(SpliceOperands::from_intent(&intent))
    }

    fn emit_effect(&self, effect: CpEffect, sid: SessionId, arg: u32) {
        let event_id = match effect {
            CpEffect::SpliceBegin => ids::SPLICE_BEGIN,
            CpEffect::SpliceCommit => ids::SPLICE_COMMIT,
            _ => effect.to_tap_event_id(),
        };
        emit(
            self.tap(),
            RawEvent::new(self.clock.now32(), event_id)
                .with_arg0(sid.raw())
                .with_arg1(arg),
        );
    }

    fn emit_policy_event(&self, id: u16, lane: Option<Lane>, arg0: u32, arg1: u32) {
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
                .with_arg1(arg1),
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

    fn policy_cancel(&self, sid: SessionId, lane: Lane) {
        self.cancel_begin_at_lane(sid, lane);
        let generation = self.r#gen.last(lane).unwrap_or(Generation(0));
        let _ = self.eval_effect(
            CpEffect::CancelAck,
            EffectContext::new(sid, lane).with_generation(generation),
        );
    }

    fn apply_policy_action(
        &self,
        action: crate::epf::Action,
        sid: Option<SessionId>,
        lane: Option<Lane>,
    ) -> Result<(), CpError> {
        if let Some(info) = action.abort_info() {
            self.handle_policy_abort(info, sid, lane);
            return Err(CpError::PolicyAbort {
                reason: info.reason,
            });
        }
        if let Some((id, arg0, arg1)) = action.tap_payload() {
            self.emit_policy_event(id, lane, arg0, arg1);
        }
        Ok(())
    }

    fn handle_policy_abort(
        &self,
        info: crate::epf::AbortInfo,
        sid: Option<SessionId>,
        lane: Option<Lane>,
    ) {
        if let Some(sid_val) = sid {
            if let Some(lane_val) = lane {
                self.policy_cancel(sid_val, lane_val);
            }
            if info.trap.is_some() {
                self.emit_policy_event(policy_trap(), lane, info.reason as u32, sid_val.raw());
            }
            self.emit_policy_event(policy_abort(), lane, info.reason as u32, sid_val.raw());
        } else {
            if info.trap.is_some() {
                self.emit_policy_event(policy_trap(), lane, info.reason as u32, 0);
            }
            self.emit_policy_event(policy_abort(), lane, info.reason as u32, 0);
        }
    }

    fn perform_effect(&self, envelope: CpCommand) -> Result<(), CpError> {
        match envelope.effect {
            CpEffect::SpliceBegin => {
                let sid = envelope.sid.ok_or(CpError::Splice(
                    crate::control::cluster::error::SpliceError::InvalidSession,
                ))?;
                let lane = envelope.lane.ok_or(CpError::Splice(
                    crate::control::cluster::error::SpliceError::InvalidLane,
                ))?;
                let (generation_input, fences) = if let Some(operands) = envelope.splice {
                    (operands.new_gen, Some((operands.seq_tx, operands.seq_rx)))
                } else {
                    (
                        envelope.generation.ok_or(CpError::Splice(
                            crate::control::cluster::error::SpliceError::GenerationMismatch,
                        ))?,
                        None,
                    )
                };
                let sid = SessionId::new(sid.raw());
                let lane = Lane::new(lane.raw());
                let generation = Generation(generation_input.raw());
                self.begin_splice(sid, lane, fences, generation)
                    .map_err(map_splice_error)
            }
            CpEffect::SpliceAck => {
                let sid = envelope.sid.ok_or(CpError::Splice(
                    crate::control::cluster::error::SpliceError::InvalidSession,
                ))?;
                let Some(operands) = envelope.splice else {
                    let lane = envelope.lane.ok_or(CpError::Splice(
                        crate::control::cluster::error::SpliceError::InvalidLane,
                    ))?;
                    let sid = SessionId::new(sid.raw());
                    let lane = Lane::new(lane.raw());
                    return match self
                        .eval_effect(CpEffect::SpliceAck, EffectContext::new(sid, lane))
                    {
                        Ok(_) => Ok(()),
                        Err(EffectError::Splice(err)) => Err(map_splice_error(err)),
                        Err(EffectError::MissingGeneration) | Err(EffectError::Rollback(_)) => {
                            Err(CpError::Splice(
                                crate::control::cluster::error::SpliceError::InvalidState,
                            ))
                        }
                        Err(EffectError::Unsupported) | Err(EffectError::Delegation(_)) => {
                            Err(CpError::UnsupportedEffect(CpEffect::SpliceAck as u8))
                        }
                        Err(EffectError::Commit(_)) => {
                            Err(CpError::UnsupportedEffect(CpEffect::SpliceAck as u8))
                        }
                    };
                };
                let intent = operands.intent(sid);
                let ack_expected = operands.ack(sid);

                let ack_result = self
                    .process_splice_intent(&intent)
                    .map_err(map_splice_error)?;

                if ack_result != ack_expected {
                    return Err(CpError::Splice(
                        crate::control::cluster::error::SpliceError::GenerationMismatch,
                    ));
                }

                let dst_lane = Lane::new(intent.dst_lane.raw());
                let sid = SessionId::new(intent.sid);
                self.assoc.register(dst_lane, sid);
                self.splice
                    .commit(dst_lane, sid)
                    .map_err(map_splice_error)?;
                Ok(())
            }
            CpEffect::SpliceCommit => {
                let sid = envelope.sid.ok_or(CpError::Splice(
                    crate::control::cluster::error::SpliceError::InvalidSession,
                ))?;
                let lane = envelope.lane.ok_or(CpError::Splice(
                    crate::control::cluster::error::SpliceError::InvalidLane,
                ))?;
                let sid = SessionId::new(sid.raw());
                let lane = Lane::new(lane.raw());
                let Some(operands) = envelope.splice else {
                    self.commit_splice(sid, lane).map_err(map_splice_error)?;
                    return Ok(());
                };
                self.commit_splice(sid, lane).map_err(map_splice_error)?;
                let released_lane = Lane::new(operands.src_lane.raw());
                if let Some(released_sid) = self.release_lane(released_lane) {
                    self.emit_lane_release(released_sid, released_lane);
                }
                Ok(())
            }
            CpEffect::Delegate => {
                let delegate = envelope.delegate.ok_or(CpError::Delegation(
                    crate::control::cluster::error::DelegationError::InvalidToken,
                ))?;

                let header = delegate.token.header();
                let sid_raw = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);
                let lane_raw = header[4] as u32;

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

                match self.eval_effect(CpEffect::Delegate, ctx) {
                    Ok(_) => Ok(()),
                    Err(EffectError::Delegation(err)) => Err(map_delegate_error(err)),
                    Err(EffectError::Unsupported) => {
                        Err(CpError::UnsupportedEffect(CpEffect::Delegate as u8))
                    }
                    Err(EffectError::Splice(_))
                    | Err(EffectError::MissingGeneration)
                    | Err(EffectError::Rollback(_))
                    | Err(EffectError::Commit(_)) => Err(CpError::Delegation(
                        crate::control::cluster::error::DelegationError::InvalidToken,
                    )),
                }
            }
            CpEffect::Commit => {
                let sid = envelope.sid.ok_or(CpError::Commit(
                    crate::control::cluster::error::CommitError::SessionNotFound,
                ))?;
                let lane = envelope.lane.ok_or(CpError::Commit(
                    crate::control::cluster::error::CommitError::SessionNotFound,
                ))?;
                let generation_input = envelope.generation.ok_or(CpError::Commit(
                    crate::control::cluster::error::CommitError::GenerationMismatch,
                ))?;
                let sid = SessionId::new(sid.raw());
                let lane = Lane::new(lane.raw());
                if self.assoc.get_sid(lane) != Some(sid) {
                    return Err(CpError::Commit(
                        crate::control::cluster::error::CommitError::SessionNotFound,
                    ));
                }
                self.commit_at_lane(sid, lane, Generation(generation_input.raw()))
                    .map_err(map_commit_error)
            }
            CpEffect::CancelBegin => {
                let sid = envelope.sid.ok_or(CpError::Cancel(
                    crate::control::cluster::error::CancelError::SessionNotFound,
                ))?;
                self.cancel_begin(SessionId::new(sid.raw()))
                    .map_err(map_cancel_error)
            }
            CpEffect::CancelAck => {
                let sid = envelope.sid.ok_or(CpError::Cancel(
                    crate::control::cluster::error::CancelError::SessionNotFound,
                ))?;
                let generation_input = envelope.generation.ok_or(CpError::Cancel(
                    crate::control::cluster::error::CancelError::GenerationMismatch,
                ))?;
                self.cancel_ack(
                    SessionId::new(sid.raw()),
                    Generation(generation_input.raw()),
                )
                .map_err(map_cancel_error)
            }
            CpEffect::Checkpoint => {
                let sid = envelope.sid.ok_or(CpError::Checkpoint(
                    crate::control::cluster::error::CheckpointError::SessionNotFound,
                ))?;
                self.checkpoint(SessionId::new(sid.raw()))
                    .map(|_| ())
                    .map_err(map_checkpoint_error)
            }
            CpEffect::Rollback => {
                let sid = envelope.sid.ok_or(CpError::Rollback(
                    crate::control::cluster::error::RollbackError::SessionNotFound,
                ))?;
                let generation_input = envelope.generation.ok_or(CpError::Rollback(
                    crate::control::cluster::error::RollbackError::EpochMismatch,
                ))?;
                self.rollback(
                    SessionId::new(sid.raw()),
                    Generation(generation_input.raw()),
                )
                .map_err(map_rollback_error)
            }
            _ => Err(CpError::UnsupportedEffect(envelope.effect as u8)),
        }
    }

    fn eval_effect(
        &self,
        effect: CpEffect,
        ctx: EffectContext,
    ) -> Result<EffectResult, EffectError> {
        match effect {
            CpEffect::SpliceBegin => {
                let target = ctx.generation.ok_or(EffectError::MissingGeneration)?;
                let mut prev = self.r#gen.last(ctx.lane);
                if prev.is_none() {
                    let _ = self.r#gen.check_and_update(ctx.lane, Generation(0));
                    prev = Some(Generation(0));
                }
                let prev = prev.unwrap_or(Generation(0));

                self.validate_splice_generation(ctx.lane, target)
                    .map_err(EffectError::Splice)?;

                let txn: Txn<LocalSpliceInvariant, IncreasingGen, One> =
                    unsafe { Txn::new(ctx.lane, prev) };
                let mut tap = NoopTap;
                let in_begin = txn.begin(&mut tap);
                let in_acked = in_begin.ack(&mut tap);

                let pending = PendingSplice::new(ctx.sid, target, in_acked, ctx.fences);

                self.splice
                    .begin(ctx.lane, pending)
                    .map_err(EffectError::Splice)?;

                let packed = ((ctx.lane.as_wire() as u32) & 0xFF) | ((target.0 as u32) << 16);
                self.emit_effect(effect, ctx.sid, packed);
                Ok(EffectResult::Generation(target))
            }
            CpEffect::SpliceAck => Ok(EffectResult::None),
            CpEffect::SpliceCommit => {
                let pending = self.splice.take(ctx.lane).ok_or(EffectError::Splice(
                    SpliceError::NoPending { lane: ctx.lane },
                ))?;

                let (sid, target, state, fences) = pending.into_parts();

                if sid != ctx.sid {
                    // Reinsert to preserve state before returning error.
                    let _ = self
                        .splice
                        .begin(ctx.lane, PendingSplice::new(sid, target, state, fences));
                    return Err(EffectError::Splice(SpliceError::UnknownSession {
                        sid: ctx.sid,
                    }));
                }

                self.validate_splice_generation(ctx.lane, target)
                    .map_err(EffectError::Splice)?;

                if let Err(err) = self.r#gen.check_and_update(ctx.lane, target) {
                    let _ = self
                        .splice
                        .begin(ctx.lane, PendingSplice::new(sid, target, state, fences));
                    let splice_err = match err {
                        GenError::StaleOrDuplicate(GenerationRecord { lane, last, new }) => {
                            SpliceError::StaleGeneration { lane, last, new }
                        }
                        GenError::Overflow { lane, last } => {
                            SpliceError::GenerationOverflow { lane, last }
                        }
                        GenError::InvalidInitial { lane, new } => {
                            SpliceError::InvalidInitial { lane, new }
                        }
                    };
                    return Err(EffectError::Splice(splice_err));
                }

                let mut tap = NoopTap;
                let _closed = state.commit(&mut tap);

                let packed = ((ctx.lane.as_wire() as u32) & 0xFF) | ((target.0 as u32) << 16);
                self.emit_effect(effect, ctx.sid, packed);
                Ok(EffectResult::Generation(target))
            }
            CpEffect::Delegate => {
                let Some(delegate) = ctx.delegate else {
                    return Err(EffectError::Unsupported);
                };

                let token = delegate.token;
                let header = token.header();
                let nonce = token.nonce();

                let sid_raw = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);
                let lane_raw = header[4] as u32;
                let role = header[5];
                let kind_raw = header[6];
                let shot_raw = header[7];

                if sid_raw != ctx.sid.raw() || lane_raw != ctx.lane.raw() {
                    return Err(EffectError::Delegation(super::error::CapError::Mismatch));
                }

                let cp_shot = crate::control::cap::mint::CapShot::from_u8(shot_raw)
                    .ok_or(EffectError::Delegation(super::error::CapError::Mismatch))?;
                if kind_raw != ENDPOINT_TAG {
                    return Err(EffectError::Delegation(super::error::CapError::Mismatch));
                }
                let shot = match cp_shot {
                    crate::control::cap::mint::CapShot::One => CapShot::One,
                    crate::control::cap::mint::CapShot::Many => CapShot::Many,
                };

                if !delegate.claim {
                    emit(
                        self.tap(),
                        DelegBegin::new(
                            self.clock.now32(),
                            ctx.sid.raw(),
                            ctx.lane.as_wire() as u32,
                        ),
                    );
                }

                if !delegate.claim {
                    let mut handle = EndpointHandle::new(
                        crate::control::types::SessionId::new(ctx.sid.raw()),
                        ctx.lane,
                        role,
                    );
                    self.mint_cap::<EndpointResource>(ctx.sid, ctx.lane, shot, role, nonce, handle);
                    EndpointResource::zeroize(&mut handle);
                    Ok(EffectResult::None)
                } else {
                    self.claim_cap(&token)
                        .map(|_cap| EffectResult::None)
                        .map_err(EffectError::Delegation)
                }
            }
            CpEffect::Commit => {
                let generation = ctx.generation.ok_or(EffectError::MissingGeneration)?;
                let checkpoint = self.checkpoints.last(ctx.lane).ok_or(EffectError::Commit(
                    CommitError::NoCheckpoint { sid: ctx.sid },
                ))?;

                if self.checkpoints.is_consumed(ctx.lane) {
                    return Err(EffectError::Commit(CommitError::AlreadyCommitted {
                        sid: ctx.sid,
                    }));
                }

                if checkpoint != generation {
                    return Err(EffectError::Commit(CommitError::GenerationMismatch {
                        sid: ctx.sid,
                        expected: checkpoint,
                        got: generation,
                    }));
                }

                self.checkpoints.mark_consumed(ctx.lane);
                self.emit_effect(effect, ctx.sid, generation.0 as u32);
                Ok(EffectResult::Generation(generation))
            }
            CpEffect::CancelBegin => {
                self.emit_effect(effect, ctx.sid, ctx.lane.as_wire() as u32);
                Ok(EffectResult::None)
            }
            CpEffect::CancelAck => {
                let generation = ctx.generation.ok_or(EffectError::MissingGeneration)?;
                self.emit_effect(effect, ctx.sid, generation.0 as u32);
                Ok(EffectResult::None)
            }
            CpEffect::Checkpoint => {
                let epoch = self.r#gen.last(ctx.lane).unwrap_or(Generation(0));
                self.checkpoints.record(ctx.lane, epoch);
                self.emit_effect(effect, ctx.sid, epoch.0 as u32);
                Ok(EffectResult::Generation(epoch))
            }
            CpEffect::Rollback => {
                let requested = ctx.generation.ok_or(EffectError::MissingGeneration)?;
                let current = self.r#gen.last(ctx.lane).unwrap_or(Generation(0));
                let checkpoint = self.checkpoints.last(ctx.lane).ok_or({
                    EffectError::Rollback(RollbackError::NoCheckpoint { sid: ctx.sid })
                })?;

                if self.checkpoints.is_consumed(ctx.lane) {
                    return Err(EffectError::Rollback(RollbackError::AlreadyConsumed {
                        sid: ctx.sid,
                    }));
                }

                if requested != checkpoint {
                    return Err(EffectError::Rollback(RollbackError::StaleCheckpoint {
                        sid: ctx.sid,
                        requested,
                        current: checkpoint,
                    }));
                }

                if current != requested {
                    return Err(EffectError::Rollback(RollbackError::EpochMismatch {
                        expected: current,
                        got: requested,
                    }));
                }

                self.checkpoints.mark_consumed(ctx.lane);

                self.emit_effect(effect, ctx.sid, requested.0 as u32);
                emit(
                    self.tap(),
                    RollbackOk::new(self.clock.now32(), ctx.sid.raw(), requested.0 as u32),
                );

                Ok(EffectResult::Generation(requested))
            }
            _ => Err(EffectError::Unsupported),
        }
    }

    #[inline]
    pub(crate) fn caps_mask_for_lane(&self, lane: Lane) -> CapsMask {
        self.vm_caps.get(lane)
    }

    #[inline]
    pub(crate) fn set_caps_mask_for_lane(&self, lane: Lane, caps: CapsMask) {
        self.vm_caps.set(lane, caps);
    }
}

impl<'rv, 'cfg, T: Transport, U: LabelUniverse, C: Clock>
    Rendezvous<'rv, 'cfg, T, U, C, crate::control::cap::mint::EpochTbl>
where
    'cfg: 'rv,
{
    const IMAGE_BANK_EXPANSION_TAIL_FLOOR: usize = 4 * 1024;
    const PUBLIC_ENDPOINT_ATTACH_TAIL_FLOOR: usize = {
        let arena_layout = crate::endpoint::kernel::EndpointArenaLayout::new(
            crate::global::role_program::MAX_LANES,
            crate::global::role_program::MAX_LANES,
            crate::endpoint::kernel::MAX_ROUTE_ARM_STACK,
            crate::eff::meta::MAX_EFF_NODES,
            crate::global::role_program::MAX_LANES,
        );
        crate::endpoint::kernel::cursor_endpoint_storage_layout::<
            0,
            T,
            U,
            C,
            crate::control::cap::mint::EpochTbl,
            1,
            crate::control::cap::mint::MintConfig,
            crate::binding::BindingHandle<'cfg>,
        >(&arena_layout, crate::global::role_program::MAX_LANES)
        .total_bytes()
    };

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

        let program_offset = Self::align_up(base, core::mem::align_of::<ProgramImageSlot>());
        let program_bytes = image_slots.checked_mul(core::mem::size_of::<ProgramImageSlot>())?;
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
        *mut ProgramImageSlot,
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
        let program_ptr = unsafe { base.add(program_offset).cast::<ProgramImageSlot>() };
        let role_ptr = unsafe { base.add(role_offset).cast::<RoleImageSlot>() };
        let lease_ptr = unsafe { base.add(lease_offset).cast::<EndpointLeaseSlot>() };

        let mut idx = 0usize;
        while idx < image_slots {
            unsafe {
                program_ptr.add(idx).write(ProgramImageSlot::EMPTY);
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
        *mut ProgramImageSlot,
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
        if slab.len().saturating_sub(baseline.3 as usize) < Self::IMAGE_BANK_EXPANSION_TAIL_FLOOR {
            return Some(baseline);
        }

        let desired = Self::recommended_image_slot_capacity(endpoint_slots);
        let mut image_slots = desired;
        loop {
            if let Some(layout) =
                Self::runtime_metadata_layout_with_image_slots(slab, endpoint_slots, image_slots)
            {
                if slab.len().saturating_sub(layout.3 as usize)
                    >= Self::IMAGE_BANK_EXPANSION_TAIL_FLOOR
                {
                    return Some(layout);
                }
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
        *mut ProgramImageSlot,
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
    ) -> Option<(
        *mut ProgramImageSlot,
        *mut RoleImageSlot,
        *mut EndpointLeaseSlot,
        u32,
        u8,
        EndpointLeaseId,
    )> {
        let required_tail = Self::IMAGE_BANK_EXPANSION_TAIL_FLOOR
            .saturating_add(Self::PUBLIC_ENDPOINT_ATTACH_TAIL_FLOOR);
        let baseline = Self::runtime_metadata_layout_for_public_path(slab, 0)?;
        let mut best = baseline;
        let per_endpoint_bytes = core::mem::size_of::<ProgramImageSlot>()
            .saturating_add(core::mem::size_of::<RoleImageSlot>())
            .saturating_add(core::mem::size_of::<EndpointLeaseSlot>());
        let per_endpoint_bytes = core::cmp::max(per_endpoint_bytes, 1);
        let mut low = 1usize;
        let mut high = core::cmp::min(
            usize::from(EndpointLeaseId::MAX),
            slab.len() / per_endpoint_bytes,
        );
        while low <= high {
            let mid = low + (high - low) / 2;
            if let Some(layout) = Self::runtime_metadata_layout_for_public_path(slab, mid) {
                if slab.len().saturating_sub(layout.3 as usize) >= required_tail {
                    best = layout;
                    low = mid.saturating_add(1);
                } else if mid == 0 {
                    break;
                } else {
                    high = mid - 1;
                }
            } else if mid == 0 {
                break;
            } else {
                high = mid - 1;
            }
        }
        unsafe { Self::init_runtime_metadata_with_image_slots(slab, usize::from(best.5), best.4) }
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
            CheckpointTable::init_empty(core::ptr::addr_of_mut!((*dst).checkpoints));
            SpliceStateTable::init_empty(core::ptr::addr_of_mut!((*dst).splice));
            DistributedSpliceTable::init_empty(core::ptr::addr_of_mut!((*dst).distributed_splice));
            core::ptr::addr_of_mut!((*dst).cap_nonce).write(Cell::new(0));
            CapTable::init_empty(core::ptr::addr_of_mut!((*dst).caps));
            LoopTable::init_empty(core::ptr::addr_of_mut!((*dst).loops));
            RouteTable::init_empty(core::ptr::addr_of_mut!((*dst).routes));
            PolicyTable::init_empty(core::ptr::addr_of_mut!((*dst).policies));
            VmCapsTable::init_empty(core::ptr::addr_of_mut!((*dst).vm_caps));
            SlotArena::init_empty(core::ptr::addr_of_mut!((*dst).slot_arena));
            HostSlots::init_empty(core::ptr::addr_of_mut!((*dst).host_slots));
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
            CheckpointTable::init_empty(core::ptr::addr_of_mut!((*dst).checkpoints));
            SpliceStateTable::init_empty(core::ptr::addr_of_mut!((*dst).splice));
            DistributedSpliceTable::init_empty(core::ptr::addr_of_mut!((*dst).distributed_splice));
            core::ptr::addr_of_mut!((*dst).cap_nonce).write(Cell::new(0));
            CapTable::init_empty(core::ptr::addr_of_mut!((*dst).caps));
            LoopTable::init_empty(core::ptr::addr_of_mut!((*dst).loops));
            RouteTable::init_empty(core::ptr::addr_of_mut!((*dst).routes));
            PolicyTable::init_empty(core::ptr::addr_of_mut!((*dst).policies));
            VmCapsTable::init_empty(core::ptr::addr_of_mut!((*dst).vm_caps));
            SlotArena::init_empty(core::ptr::addr_of_mut!((*dst).slot_arena));
            HostSlots::init_empty(core::ptr::addr_of_mut!((*dst).host_slots));
            core::ptr::addr_of_mut!((*dst).clock).write(clock);
            core::ptr::addr_of_mut!((*dst).liveness_policy).write(liveness_policy);
            core::ptr::addr_of_mut!((*dst)._epoch_marker).write(PhantomData);
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
            Self::init_runtime_metadata_for_public_path_auto(runtime_slab).unwrap_or((
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
            CheckpointTable::init_empty(core::ptr::addr_of_mut!((*dst).checkpoints));
            SpliceStateTable::init_empty(core::ptr::addr_of_mut!((*dst).splice));
            DistributedSpliceTable::init_empty(core::ptr::addr_of_mut!((*dst).distributed_splice));
            core::ptr::addr_of_mut!((*dst).cap_nonce).write(Cell::new(0));
            CapTable::init_empty(core::ptr::addr_of_mut!((*dst).caps));
            LoopTable::init_empty(core::ptr::addr_of_mut!((*dst).loops));
            RouteTable::init_empty(core::ptr::addr_of_mut!((*dst).routes));
            PolicyTable::init_empty(core::ptr::addr_of_mut!((*dst).policies));
            VmCapsTable::init_empty(core::ptr::addr_of_mut!((*dst).vm_caps));
            SlotArena::init_empty(core::ptr::addr_of_mut!((*dst).slot_arena));
            HostSlots::init_empty(core::ptr::addr_of_mut!((*dst).host_slots));
            core::ptr::addr_of_mut!((*dst).clock).write(clock);
            core::ptr::addr_of_mut!((*dst).liveness_policy).write(liveness_policy);
            core::ptr::addr_of_mut!((*dst)._epoch_marker).write(PhantomData);
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
            ControlScopeKind::Checkpoint => {
                self.checkpoints.reset_lane(lane);
            }
            ControlScopeKind::Cancel => {}
            ControlScopeKind::Splice => {
                self.splice.reset_lane(lane);
            }
            ControlScopeKind::Reroute
            | ControlScopeKind::Policy
            | ControlScopeKind::Route
            | ControlScopeKind::None => {}
        }
    }

    #[inline]
    pub(crate) fn checkpoint_at_lane(&self, sid: SessionId, lane: Lane) -> Generation {
        match self.eval_effect(CpEffect::Checkpoint, EffectContext::new(sid, lane)) {
            Ok(EffectResult::Generation(epoch)) => epoch,
            Ok(EffectResult::None) => unreachable!("checkpoint effect must yield generation"),
            Err(_) => unreachable!("checkpoint effect cannot fail"),
        }
    }

    #[inline]
    pub(crate) fn commit_at_lane(
        &self,
        sid: SessionId,
        lane: Lane,
        generation: Generation,
    ) -> Result<(), CommitError> {
        match self.eval_effect(
            CpEffect::Commit,
            EffectContext::new(sid, lane).with_generation(generation),
        ) {
            Ok(_) => Ok(()),
            Err(EffectError::Commit(err)) => Err(err),
            Err(EffectError::MissingGeneration)
            | Err(EffectError::Unsupported)
            | Err(EffectError::Splice(_))
            | Err(EffectError::Delegation(_))
            | Err(EffectError::Rollback(_)) => {
                unreachable!("commit effect failure is fully covered")
            }
        }
    }

    #[inline]
    pub(crate) fn cancel_begin_at_lane(&self, sid: SessionId, lane: Lane) {
        self.eval_effect(CpEffect::CancelBegin, EffectContext::new(sid, lane))
            .expect("cancel begin evaluation must not fail");
    }

    pub(crate) fn is_session_registered(&self, sid: SessionId) -> bool {
        self.assoc.find_lane(sid).is_some()
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
        self.checkpoints.reset_lane(lane);
        self.splice.reset_lane(lane);
        self.caps.purge_lane(lane);
        self.loops.reset_lane(lane);
        self.routes.reset_lane(lane);
        self.vm_caps.reset_lane(lane);
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
pub(crate) struct LaneLease<'cfg, T, U, C, const MAX_RV: usize>
where
    T: Transport,
    U: LabelUniverse + 'cfg,
    C: Clock + 'cfg,
{
    /// Lease-backed guard over the parent rendezvous.
    /// Uses the default EpochTbl because LaneLease is only used to create new endpoints.
    guard: Option<LaneGuard<'cfg, T, U, C>>,
    /// Session identifier.
    sid: SessionId,
    /// Lane identifier.
    lane: Lane,
    /// Role for the port.
    role: u8,
    /// Number of global roles participating in the attached program.
    role_count: u8,
    /// Rendezvous brand for typed owner construction.
    brand: crate::control::brand::Guard<'cfg>,
}

impl<'cfg, T, U, C, const MAX_RV: usize> LaneLease<'cfg, T, U, C, MAX_RV>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
{
    /// Internal constructor (called by `SessionKit::lease_port`).
    /// The caller must ensure no duplicate leases for the same `(rv_id, lane)` pair.
    pub(crate) fn new(
        guard: LaneGuard<'cfg, T, U, C>,
        sid: SessionId,
        lane: Lane,
        role: u8,
        role_count: u8,
        brand: crate::control::brand::Guard<'cfg>,
    ) -> Self {
        Self {
            guard: Some(guard),
            sid,
            lane,
            role,
            role_count,
            brand,
        }
    }

    #[allow(clippy::type_complexity)]
    pub(crate) fn into_port_guard(
        mut self,
    ) -> Result<
        (
            Port<'cfg, T, crate::control::cap::mint::EpochTbl>,
            LaneGuard<'cfg, T, U, C>,
            crate::control::brand::Guard<'cfg>,
        ),
        RendezvousError,
    > {
        let mut guard = self.guard.take().expect("lane lease retains guard");
        let port = {
            let lease_ref = guard.lease.as_mut().expect("guard retains lease");
            let rv_ptr: *mut Rendezvous<'cfg, 'cfg, T, U, C, crate::control::cap::mint::EpochTbl> =
                lease_ref.with_rendezvous(core::ptr::from_mut);
            // SAFETY: `LaneLease` holds the unique rendezvous lease while the guard
            // is alive, so the rendezvous cannot move or be aliased here.
            let rv: &'cfg Rendezvous<'cfg, 'cfg, T, U, C, crate::control::cap::mint::EpochTbl> =
                unsafe { &*rv_ptr };
            rv.acquire_port(self.sid, self.lane, self.role, self.role_count)?
        };
        guard.detach_lease();
        Ok((port, guard, self.brand))
    }
}

impl<'cfg, T, U, C, const MAX_RV: usize> Drop for LaneLease<'cfg, T, U, C, MAX_RV>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
{
    fn drop(&mut self) {
        if let Some(guard) = self.guard.take() {
            drop(guard);
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

    /// Release a capability from the CapTable by nonce.
    #[inline]
    pub(crate) fn release_cap_by_nonce(
        &self,
        nonce: &[u8; crate::control::cap::mint::CAP_NONCE_LEN],
    ) {
        self.caps.release_by_nonce(nonce);
    }

    pub(crate) fn acquire_port<'a>(
        &'a self,
        sid: SessionId,
        lane: Lane,
        role: u8,
        role_count: u8,
    ) -> Result<Port<'a, T, crate::control::cap::mint::EpochTbl>, RendezvousError>
    where
        'rv: 'a,
    {
        if !self.lane_range.contains(&lane.0) {
            return Err(RendezvousError::LaneOutOfRange { lane });
        }
        let first_attach = match self.assoc.get_sid(lane) {
            None => {
                self.assoc.register(lane, sid);
                true
            }
            Some(existing) if existing == sid => {
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
            // Emit CpEffect::Open for the lane's inaugural attachment.
            emit(
                self.tap(),
                RawEvent::new(
                    self.clock.now32(),
                    crate::control::cluster::effects::CpEffect::Open.to_tap_event_id(),
                )
                .with_arg0(sid.raw())
                .with_arg1(lane.0),
            );

            self.r#gen.reset_lane(lane);
            self.checkpoints.reset_lane(lane);
            self.loops.reset_lane(lane);
            self.routes.reset_lane(lane);
        }
        let (tx, rx) = self.transport.open(role, sid.raw());
        Ok(Port::new(
            &self.transport,
            self.tap(),
            &self.clock,
            &self.vm_caps,
            &self.loops,
            &self.routes,
            &self.host_slots,
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
        ))
    }

    // ============================================================================
    // Capability methods
    // ============================================================================

    #[inline]
    pub(crate) fn next_nonce_seed(&self) -> NonceSeed {
        let ordinal = self.cap_nonce.get();
        self.cap_nonce.set(ordinal.wrapping_add(1));
        NonceSeed::counter(ordinal as u64)
    }

    pub(crate) fn mint_cap<K: ResourceKind>(
        &self,
        sid: SessionId,
        lane: Lane,
        shot: CapShot,
        dest_role: u8,
        nonce: [u8; 16],
        mut handle: K::Handle,
    ) {
        let kind_tag = K::TAG;
        let registered_sid = self
            .assoc
            .get_sid(lane)
            .expect("session must be registered before minting capabilities");
        debug_assert_eq!(
            registered_sid, sid,
            "capabilities must be minted on a lane registered to the session"
        );
        debug_assert!(
            self.assoc.is_active(lane),
            "lane must be active before minting capabilities"
        );

        let handle_bytes = K::encode_handle(&handle);
        K::zeroize(&mut handle);

        let entry = CapEntry {
            sid,
            lane_raw: lane.as_wire(),
            kind_tag,
            shot_state: shot.as_u8(),
            role: dest_role,
            nonce,
            handle: handle_bytes,
        };
        self.caps
            .insert_entry(entry)
            .expect("capability table is full");

        let tap = self.tap();
        emit(
            tap,
            RawEvent::new(self.clock.now32(), crate::observe::cap_mint::<K>())
                .with_arg0(sid.raw())
                .with_arg1(((lane.as_wire() as u32) << 16) | (dest_role as u32)),
        );
    }

    pub(crate) fn claim_cap<K: crate::control::cap::mint::ResourceKind>(
        &self,
        token: &GenericCapToken<K>,
    ) -> Result<VerifiedCap<K>, CapError> {
        // Extract fields from 40B token
        let header = token.header();
        let nonce = token.nonce();

        let sid = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);
        let lane = header[4];
        let role = header[5];
        let kind_tag = header[6];
        let shot_u8 = header[7];

        let sid = SessionId(sid);
        let lane = Lane(lane as u32);
        let shot = CapShot::from_u8(shot_u8).ok_or(CapError::UnknownToken)?;

        // Check if AUTO (all zeros)
        if nonce == [0u8; crate::control::cap::mint::CAP_NONCE_LEN]
            && header == [0u8; crate::control::cap::mint::CAP_HEADER_LEN]
        {
            return Err(CapError::UnknownToken);
        }

        if self.assoc.get_sid(lane) != Some(sid) {
            return Err(CapError::WrongSessionOrLane);
        }

        if kind_tag != K::TAG {
            return Err(CapError::Mismatch);
        }

        // Use nonce-based claim path (trusted domain - no MAC verification)
        let (exhausted, handle_bytes) = self
            .caps
            .claim_by_nonce(&nonce, sid, lane, kind_tag, role, shot, token.caps_mask())
            .map_err(|e| match e {
                CapError::UnknownToken => CapError::UnknownToken,
                CapError::WrongSessionOrLane => CapError::WrongSessionOrLane,
                CapError::Exhausted => CapError::Exhausted,
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

    // ============================================================================
    // Distributed splice methods
    // ============================================================================

    pub(crate) fn begin_distributed_splice(
        &self,
        sid: SessionId,
        src_lane: Lane,
        dst_rv: RendezvousId,
        dst_lane: Lane,
        fences: Option<(u32, u32)>,
    ) -> Result<SpliceIntent, SpliceError> {
        // Verify session exists and is on the expected lane
        if self.assoc.get_sid(src_lane) != Some(sid) {
            return Err(SpliceError::UnknownSession { sid });
        }

        // Get current generation and calculate next
        let old_gen = self.r#gen.last(src_lane).unwrap_or(Generation(0));
        let new_gen = Generation(old_gen.0.saturating_add(1));

        if new_gen.0 == 0 {
            return Err(SpliceError::GenerationOverflow {
                lane: src_lane,
                last: old_gen,
            });
        }

        let intent = SpliceIntent::new(
            self.id,
            dst_rv,
            sid.raw(),
            old_gen,
            new_gen,
            fences.map(|f| f.0).unwrap_or(0),
            fences.map(|f| f.1).unwrap_or(0),
            src_lane,
            dst_lane,
        );

        // Store intent locally
        self.distributed_splice.insert(intent)?;

        // Emit tap event
        emit(
            self.tap(),
            DelegSplice::new(
                self.clock.now32(),
                src_lane.0 | (dst_lane.0 << 8) | ((new_gen.0 as u32) << 16),
                sid.raw(),
            ),
        );

        Ok(intent)
    }

    pub(crate) fn take_cached_distributed_intent(
        &self,
        sid: SessionId,
        dst_rv: RendezvousId,
    ) -> Option<SpliceIntent> {
        self.distributed_splice
            .take(sid, self.id, dst_rv)
            .map(|entry| entry.intent)
    }

    pub(crate) fn process_splice_intent(
        &self,
        intent: &SpliceIntent,
    ) -> Result<SpliceAck, SpliceError> {
        let dst_rv: RendezvousId = intent.dst_rv;
        let dst_lane: Lane = intent.dst_lane;
        let old_gen: Generation = intent.old_gen;
        let new_gen: Generation = intent.new_gen;

        // Validate this RV is the intended destination
        if dst_rv != self.id {
            return Err(SpliceError::RendezvousIdMismatch {
                expected: dst_rv,
                got: self.id,
            });
        }

        // Validate lane is in range
        if !self.lane_range.contains(&dst_lane.raw()) {
            return Err(SpliceError::LaneOutOfRange { lane: dst_lane });
        }

        // Check lane is available
        if self.assoc.is_active(dst_lane) {
            return Err(SpliceError::LaneMismatch {
                expected: dst_lane,
                provided: Lane(0), // Dummy value
            });
        }

        // Validate generation monotonicity
        let last_gen = self.r#gen.last(dst_lane).unwrap_or(Generation(0));

        // Allow old_gen to be 0 (new session) or match the last generation
        if old_gen.0 != 0 && old_gen.0 != last_gen.0 {
            return Err(SpliceError::StaleGeneration {
                lane: dst_lane,
                last: last_gen,
                new: new_gen,
            });
        }

        // Begin local splice using typestate transaction (ack immediately for local state).
        let txn: Txn<LocalSpliceInvariant, IncreasingGen, One> =
            unsafe { Txn::new(dst_lane, last_gen) };
        let mut tap = NoopTap;
        let in_begin = txn.begin(&mut tap);
        let in_acked = in_begin.ack(&mut tap);

        let pending = PendingSplice::new(
            SessionId(intent.sid),
            new_gen,
            in_acked,
            Some((intent.seq_tx, intent.seq_rx)),
        );
        let begin_result = self.splice.begin(dst_lane, pending);
        begin_result?;

        // Update generation table
        if last_gen.0 == 0 {
            let _ = self.r#gen.check_and_update(dst_lane, Generation(0));
            self.r#gen
                .check_and_update(dst_lane, new_gen)
                .map_err(|err| match err {
                    GenError::StaleOrDuplicate(GenerationRecord { lane, last, new }) => {
                        SpliceError::StaleGeneration { lane, last, new }
                    }
                    GenError::Overflow { lane, last } => {
                        SpliceError::GenerationOverflow { lane, last }
                    }
                    GenError::InvalidInitial { lane, new } => {
                        SpliceError::InvalidInitial { lane, new }
                    }
                })?;
        } else {
            self.r#gen
                .check_and_update(dst_lane, new_gen)
                .map_err(|err| match err {
                    GenError::StaleOrDuplicate(GenerationRecord { lane, last, new }) => {
                        SpliceError::StaleGeneration { lane, last, new }
                    }
                    GenError::Overflow { lane, last } => {
                        SpliceError::GenerationOverflow { lane, last }
                    }
                    GenError::InvalidInitial { lane, new } => {
                        SpliceError::InvalidInitial { lane, new }
                    }
                })?;
        }

        // Create ack using control::automaton::distributed::SpliceAck::new
        let ack = SpliceAck::new(
            intent.src_rv,
            self.id,
            intent.sid,
            new_gen,
            dst_lane,
            intent.seq_tx,
            intent.seq_rx,
        );

        Ok(ack)
    }

    // ============================================================================
    // Checkpoint / Cancel / Rollback methods
    // ============================================================================

    pub(crate) fn cancel_begin(&self, sid: SessionId) -> Result<(), CancelError> {
        let lane = self
            .assoc
            .find_lane(sid)
            .ok_or(CancelError::UnknownSession { sid })?;
        self.cancel_begin_at_lane(sid, lane);
        Ok(())
    }

    pub(crate) fn cancel_ack(&self, sid: SessionId, r#gen: Generation) -> Result<(), CancelError> {
        let lane = self
            .assoc
            .find_lane(sid)
            .ok_or(CancelError::UnknownSession { sid })?;
        self.eval_effect(
            CpEffect::CancelAck,
            EffectContext::new(sid, lane).with_generation(r#gen),
        )
        .expect("cancel ack evaluation must not fail");
        Ok(())
    }

    pub(crate) fn checkpoint(&self, sid: SessionId) -> Result<Generation, CheckpointError> {
        let lane = self
            .assoc
            .find_lane(sid)
            .ok_or(CheckpointError::UnknownSession { sid })?;
        Ok(self.checkpoint_at_lane(sid, lane))
    }

    pub(crate) fn rollback(&self, sid: SessionId, epoch: Generation) -> Result<(), RollbackError> {
        let lane = self
            .assoc
            .find_lane(sid)
            .ok_or(RollbackError::UnknownSession { sid })?;
        self.rollback_at_lane(sid, lane, epoch)
    }

    pub(crate) fn rollback_at_lane(
        &self,
        sid: SessionId,
        lane: Lane,
        epoch: Generation,
    ) -> Result<(), RollbackError> {
        match self.eval_effect(
            CpEffect::Rollback,
            EffectContext::new(sid, lane).with_generation(epoch),
        ) {
            Ok(_) => Ok(()),
            Err(EffectError::Rollback(err)) => Err(err),
            Err(EffectError::MissingGeneration)
            | Err(EffectError::Unsupported)
            | Err(EffectError::Splice(_))
            | Err(EffectError::Delegation(_))
            | Err(EffectError::Commit(_)) => {
                unreachable!("rollback effect failure is fully covered")
            }
        }
    }

    pub(crate) fn validate_splice_generation(
        &self,
        lane: Lane,
        new_gen: Generation,
    ) -> Result<(), SpliceError> {
        match self.r#gen.last(lane) {
            None => {
                if new_gen.0 >= 1 {
                    Ok(())
                } else {
                    Err(SpliceError::InvalidInitial { lane, new: new_gen })
                }
            }
            Some(prev) if prev.0 == u16::MAX => {
                Err(SpliceError::GenerationOverflow { lane, last: prev })
            }
            Some(prev) if new_gen.0 > prev.0 => Ok(()),
            Some(prev) => Err(SpliceError::StaleGeneration {
                lane,
                last: prev,
                new: new_gen,
            }),
        }
    }
}

// ============================================================================
// SpliceDelegate trait has been DELETED.
// All splice operations now go through control::CpCommand and EffectRunner.
// The control-plane mini-kernel architecture is responsible for rendezvous access control.

fn map_splice_error(err: SpliceError) -> CpError {
    match err {
        SpliceError::LaneOutOfRange { .. } => {
            CpError::Splice(crate::control::cluster::error::SpliceError::InvalidLane)
        }
        SpliceError::LaneMismatch { .. }
        | SpliceError::InProgress { .. }
        | SpliceError::NoPending { .. }
        | SpliceError::SeqnoMismatch { .. } => {
            CpError::Splice(crate::control::cluster::error::SpliceError::InvalidState)
        }
        SpliceError::UnknownSession { .. } => {
            CpError::Splice(crate::control::cluster::error::SpliceError::InvalidSession)
        }
        SpliceError::StaleGeneration { .. }
        | SpliceError::GenerationOverflow { .. }
        | SpliceError::InvalidInitial { .. } => {
            CpError::Splice(crate::control::cluster::error::SpliceError::GenerationMismatch)
        }
        SpliceError::RemoteRendezvousMismatch { expected, got }
        | SpliceError::RendezvousIdMismatch { expected, got } => CpError::RendezvousMismatch {
            expected: expected.raw(),
            actual: got.raw(),
        },
        SpliceError::PendingTableFull => CpError::ResourceExhausted,
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
    };
    CpError::Delegation(deleg_err)
}

fn map_cancel_error(err: super::error::CancelError) -> CpError {
    match err {
        super::error::CancelError::UnknownSession { .. } => {
            CpError::Cancel(crate::control::cluster::error::CancelError::SessionNotFound)
        }
    }
}

fn map_checkpoint_error(err: super::error::CheckpointError) -> CpError {
    match err {
        super::error::CheckpointError::UnknownSession { .. } => {
            CpError::Checkpoint(crate::control::cluster::error::CheckpointError::SessionNotFound)
        }
    }
}

fn map_commit_error(err: super::error::CommitError) -> CpError {
    match err {
        super::error::CommitError::NoCheckpoint { .. } => {
            CpError::Commit(crate::control::cluster::error::CommitError::NoCheckpoint)
        }
        super::error::CommitError::AlreadyCommitted { .. } => {
            CpError::Commit(crate::control::cluster::error::CommitError::AlreadyCommitted)
        }
        super::error::CommitError::GenerationMismatch { .. } => {
            CpError::Commit(crate::control::cluster::error::CommitError::GenerationMismatch)
        }
    }
}

fn map_rollback_error(err: super::error::RollbackError) -> CpError {
    match err {
        super::error::RollbackError::UnknownSession { .. } => {
            CpError::Rollback(crate::control::cluster::error::RollbackError::SessionNotFound)
        }
        super::error::RollbackError::NoCheckpoint { .. } => {
            CpError::Rollback(crate::control::cluster::error::RollbackError::EpochNotFound)
        }
        super::error::RollbackError::StaleCheckpoint { .. }
        | super::error::RollbackError::EpochMismatch { .. } => {
            CpError::Rollback(crate::control::cluster::error::RollbackError::EpochMismatch)
        }
        super::error::RollbackError::AlreadyConsumed { .. } => {
            CpError::Rollback(crate::control::cluster::error::RollbackError::AfterCommit)
        }
    }
}

// ============================================================================
// Local splice operations (used by EffectRunner)
// ============================================================================

impl<'rv, 'cfg, T, U, C, E> Rendezvous<'rv, 'cfg, T, U, C, E>
where
    'cfg: 'rv,
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
    /// Begin a local splice operation.
    ///
    /// This is called by EffectRunner::run_effect() for CpEffect::SpliceBegin.
    fn begin_splice(
        &self,
        sid: SessionId,
        lane: Lane,
        fences: Option<(u32, u32)>,
        generation: Generation,
    ) -> Result<(), SpliceError> {
        let ctx = EffectContext::new(sid, lane)
            .with_generation(generation)
            .with_fences(fences);

        match self.eval_effect(CpEffect::SpliceBegin, ctx) {
            Ok(_) => Ok(()),
            Err(EffectError::Splice(err)) => Err(err),
            Err(EffectError::MissingGeneration)
            | Err(EffectError::Unsupported)
            | Err(EffectError::Commit(_))
            | Err(EffectError::Delegation(_))
            | Err(EffectError::Rollback(_)) => {
                unreachable!("splice begin effect failure is fully covered")
            }
        }
    }

    /// Commit a local splice operation.
    ///
    /// This is called by EffectRunner::run_effect() for CpEffect::SpliceCommit.
    fn commit_splice(&self, sid: SessionId, lane: Lane) -> Result<(), SpliceError> {
        let ctx = EffectContext::new(sid, lane);
        match self.eval_effect(CpEffect::SpliceCommit, ctx) {
            Ok(_) => Ok(()),
            Err(EffectError::Splice(err)) => Err(err),
            Err(EffectError::MissingGeneration)
            | Err(EffectError::Unsupported)
            | Err(EffectError::Delegation(_))
            | Err(EffectError::Rollback(_))
            | Err(EffectError::Commit(_)) => {
                unreachable!("splice commit failure is fully covered")
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
        let snapshot = self.transport.metrics().snapshot();
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
    fn run_effect(&self, envelope: CpCommand) -> Result<(), CpError> {
        let envelope = match envelope.effect {
            CpEffect::Delegate => envelope.canonicalize_delegate()?,
            _ => envelope,
        };
        let lane_opt = envelope.lane.map(|lane| Lane::new(lane.raw()));
        let sid_opt = envelope.sid.map(|sid| SessionId::new(sid.raw()));
        let caps_mask = lane_opt
            .map(|lane| self.vm_caps.get(lane))
            .unwrap_or(CapsMask::allow_all());

        let policy_event = RawEvent::new(self.clock.now32(), envelope.effect.to_tap_event_id())
            .with_arg0(sid_opt.map_or(0, |sid| sid.raw()))
            .with_arg1(lane_opt.map_or(0, |lane| lane.raw()));

        let handle_data = envelope.delegate.as_ref().map(|delegate| {
            (
                delegate.token.resource_tag(),
                delegate.token.handle_bytes(),
                delegate.token.caps_mask(),
            )
        });

        let _ = self.flush_transport_events();
        let transport_metrics = self.transport.metrics().snapshot();
        let policy_input =
            crate::epf::slot_contract::slot_default_input(crate::epf::vm::Slot::Rendezvous);
        let policy_digest = self.policy_digest(crate::epf::vm::Slot::Rendezvous);
        let event_hash = crate::epf::hash_tap_event(&policy_event);
        let signals_input_hash = crate::epf::hash_policy_input(policy_input);
        let transport_snapshot_hash = crate::epf::hash_transport_snapshot(transport_metrics);
        let replay_transport = crate::epf::replay_transport_inputs(transport_metrics);
        let replay_transport_presence = crate::epf::replay_transport_presence(transport_metrics);
        let mode_id =
            crate::epf::policy_mode_tag(self.policy_mode(crate::epf::vm::Slot::Rendezvous));
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
            0,
            transport_snapshot_hash,
            ((crate::epf::slot_tag(crate::epf::vm::Slot::Rendezvous) as u32) << 24)
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
        let action = self.run_policy(
            crate::epf::vm::Slot::Rendezvous,
            &policy_event,
            caps_mask,
            sid_opt,
            lane_opt,
            move |ctx| {
                let _ = handle_data;
                ctx.set_transport_snapshot(transport_metrics);
                ctx.set_policy_input(policy_input);
            },
        );
        let verdict = action.verdict();
        let verdict_meta = ((crate::epf::verdict_tag(verdict) as u32) << 24)
            | ((crate::epf::verdict_arm(verdict) as u32) << 16);
        self.emit_policy_event_with_arg2(
            ids::POLICY_AUDIT_RESULT,
            lane_opt,
            verdict_meta,
            crate::epf::verdict_reason(verdict) as u32,
            self.last_policy_fuel_used(crate::epf::vm::Slot::Rendezvous) as u32,
        );

        self.apply_policy_action(action, sid_opt, lane_opt)?;

        if !caps_mask.allows(envelope.effect) {
            return Err(CpError::Authorisation {
                effect: envelope.effect,
            });
        }

        self.perform_effect(envelope)
    }

    fn prepare_splice_operands(
        &self,
        sid: SessionId,
        src_lane: Lane,
        dst_rv: RendezvousId,
        dst_lane: Lane,
        fences: Option<(u32, u32)>,
    ) -> Result<SpliceOperands, CpError> {
        self.prepare_distributed_splice_operands(sid, src_lane, dst_rv, dst_lane, fences)
    }
}

// ============================================================================

#[cfg(test)]
mod epf_tests {
    use super::*;
    use crate::{
        control::cap::mint::CapsMask,
        control::cluster::core::{CpCommand, EffectRunner},
        control::types::{Lane, SessionId},
        g::{self, Msg, Role},
        global::{compiled::LoweringSummary, typestate::RoleCompileScratch},
        observe::core::TapEvent,
        runtime::{config::Config, consts::RING_EVENTS},
        transport::{Transport, TransportError, wire::Payload},
    };
    use core::{
        cell::UnsafeCell,
        future::{Ready, ready},
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
        type Send<'a>
            = Ready<Result<(), Self::Error>>
        where
            Self: 'a;
        type Recv<'a>
            = Ready<Result<Payload<'a>, Self::Error>>
        where
            Self: 'a;
        type Metrics = ();

        fn open<'a>(&'a self, _local_role: u8, _session_id: u32) -> (Self::Tx<'a>, Self::Rx<'a>) {
            ((), ())
        }

        fn send<'a, 'f>(
            &'a self,
            _tx: &'a mut Self::Tx<'a>,
            _outgoing: crate::transport::Outgoing<'f>,
        ) -> Self::Send<'a>
        where
            'a: 'f,
        {
            ready(Ok(()))
        }

        fn recv<'a>(&'a self, _rx: &'a mut Self::Rx<'a>) -> Self::Recv<'a> {
            ready(Err(TransportError::Offline))
        }

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

    type TestRendezvous = Rendezvous<
        'static,
        'static,
        DummyTransport,
        crate::runtime::consts::DefaultLabelUniverse,
        crate::runtime::config::CounterClock,
        crate::control::cap::mint::EpochTbl,
    >;

    thread_local! {
        static EPF_TEST_TAP: UnsafeCell<[TapEvent; RING_EVENTS]> =
            const { UnsafeCell::new([TapEvent::zero(); RING_EVENTS]) };
        static EPF_TEST_SLAB: UnsafeCell<[u8; 256]> =
            const { UnsafeCell::new([0u8; 256]) };
        static EPF_TEST_RENDEZVOUS: UnsafeCell<MaybeUninit<TestRendezvous>> =
            const { UnsafeCell::new(MaybeUninit::uninit()) };
        static IMAGE_TEST_SLAB: UnsafeCell<[u8; 32768]> =
            const { UnsafeCell::new([0u8; 32768]) };
        static IMAGE_TEST_RENDEZVOUS: UnsafeCell<MaybeUninit<TestRendezvous>> =
            const { UnsafeCell::new(MaybeUninit::uninit()) };
    }

    fn with_epf_test_rendezvous<R>(f: impl FnOnce(&mut TestRendezvous) -> R) -> R {
        EPF_TEST_TAP.with(|tap| {
            EPF_TEST_SLAB.with(|slab| {
                EPF_TEST_RENDEZVOUS.with(|rendezvous| unsafe {
                    let tap = &mut *tap.get();
                    tap.fill(TapEvent::zero());
                    let slab = &mut *slab.get();
                    slab.fill(0);
                    let config = Config::new(tap, slab);
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
                    let config = Config::new(tap, slab);
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
                let config = Config::new(tap, slab);
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

    fn route_summary() -> LoweringSummary {
        let program = g::send::<Role<0>, Role<1>, Msg<11, u32>, 0>();
        program.summary().clone()
    }

    fn route_summary_alt() -> LoweringSummary {
        let program = g::send::<Role<0>, Role<1>, Msg<12, u32>, 0>();
        program.summary().clone()
    }

    #[test]
    fn run_effect_requires_authorised_caps() {
        with_epf_test_rendezvous(|rendezvous| {
            let sid = SessionId::new(1);
            let lane = Lane::new(0);

            rendezvous.vm_caps.set(lane, CapsMask::empty());

            let envelope = CpCommand::checkpoint(SessionId::new(sid.raw()), Lane::new(lane.raw()));

            let result = EffectRunner::run_effect(rendezvous, envelope);

            assert!(matches!(
                result,
                Err(CpError::Authorisation {
                    effect: CpEffect::Checkpoint
                })
            ));
        });
    }

    #[test]
    fn run_effect_allows_when_caps_present() {
        with_epf_test_rendezvous(|rendezvous| {
            let sid = SessionId::new(2);
            let lane = Lane::new(1);

            let envelope = CpCommand::checkpoint(SessionId::new(sid.raw()), Lane::new(lane.raw()));

            let result = EffectRunner::run_effect(rendezvous, envelope);

            assert!(matches!(result, Err(CpError::Checkpoint(_))));
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

            let mut scratch = RoleCompileScratch::new();
            assert!(
                rendezvous
                    .materialize_role_image_from_summary::<0>(stamp, &summary, &mut scratch)
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

            let mut scratch = RoleCompileScratch::new();
            assert!(
                rendezvous
                    .materialize_role_image_from_summary::<0>(stamp, &summary, &mut scratch)
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
    fn splice_tables_bind_only_for_splice_control_scope() {
        with_image_test_rendezvous(|rendezvous| {
            assert!(!rendezvous.splice.is_bound());
            assert!(!rendezvous.distributed_splice.is_bound());

            rendezvous.initialise_control_scope(Lane::new(0), ControlScopeKind::Loop);
            assert!(
                !rendezvous.splice.is_bound() && !rendezvous.distributed_splice.is_bound(),
                "non-splice control scopes must not bind splice storage"
            );

            rendezvous
                .prepare_splice_control_scope(Lane::new(0))
                .expect("splice control scope should bind splice storage");
            assert!(rendezvous.splice.is_bound());
            assert!(rendezvous.distributed_splice.is_bound());
        });
    }

    #[test]
    fn trim_resident_headers_reclaims_frontier_when_no_images_remain_above_sidecars() {
        with_image_test_rendezvous(|rendezvous| {
            let initial_frontier = rendezvous.image_frontier;
            rendezvous
                .ensure_endpoint_resident_budget(EndpointResidentBudget::with_route_storage(
                    2,
                    LANES_MAX as usize,
                    3,
                    8,
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
                    2,
                    LANES_MAX as usize,
                    3,
                    8,
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
                    2,
                    LANES_MAX as usize,
                    3,
                    8,
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
            let mut scratch = RoleCompileScratch::new();
            rendezvous
                .materialize_role_image_from_summary::<0>(
                    summary_a.stamp(),
                    &summary_a,
                    &mut scratch,
                )
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
    /// Borrow capability table as a constrained facet.
    pub(crate) fn caps_facet(&mut self) -> CapsFacet<T, U, C, E> {
        CapsFacet::new()
    }

    /// Borrow splice coordination state as a constrained facet.
    pub(crate) fn splice_facet(&mut self) -> SpliceFacet<T, U, C, E> {
        SpliceFacet::new()
    }

    /// Borrow observation ring as a constrained facet.
    pub(crate) fn observe_facet(&self) -> ObserveFacet<'_, 'cfg> {
        ObserveFacet::new(self.tap())
    }
}

/// Capability-focused facet that exposes only CapTable operations.
#[derive(Default)]
pub(crate) struct CapsFacet<T, U, C, E>(PhantomData<(T, U, C, E)>)
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable;

impl<T, U, C, E> Copy for CapsFacet<T, U, C, E>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
}

impl<T, U, C, E> Clone for CapsFacet<T, U, C, E>
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

impl<T, U, C, E> CapsFacet<T, U, C, E>
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

    /// Mint a capability token and register it in the CapTable.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn mint_cap<K: crate::control::cap::mint::ResourceKind>(
        self,
        rendezvous: &Rendezvous<'_, '_, T, U, C, E>,
        sid: SessionId,
        lane: Lane,
        shot: crate::control::cap::mint::CapShot,
        dest_role: u8,
        nonce: [u8; 16],
        handle: K::Handle,
    ) {
        rendezvous.mint_cap::<K>(sid, lane, shot, dest_role, nonce, handle)
    }

    /// Generate the next nonce seed for capability minting.
    #[inline]
    pub(crate) fn next_nonce_seed(
        self,
        rendezvous: &Rendezvous<'_, '_, T, U, C, E>,
    ) -> crate::control::cap::mint::NonceSeed {
        rendezvous.next_nonce_seed()
    }
}

/// Splice-focused facet that exposes only splice coordination operations.
#[derive(Default)]
pub(crate) struct SpliceFacet<T, U, C, E>(PhantomData<(T, U, C, E)>)
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable;

impl<T, U, C, E> Copy for SpliceFacet<T, U, C, E>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
}

impl<T, U, C, E> Clone for SpliceFacet<T, U, C, E>
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

impl<T, U, C, E> SpliceFacet<T, U, C, E>
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

    pub(crate) fn begin(
        self,
        rendezvous: &mut Rendezvous<'_, '_, T, U, C, E>,
        sid: SessionId,
        lane: Lane,
        fences: Option<(u32, u32)>,
        generation: Generation,
    ) -> Result<(), super::error::SpliceError> {
        rendezvous.begin_splice(sid, lane, fences, generation)
    }

    pub(crate) fn commit(
        self,
        rendezvous: &mut Rendezvous<'_, '_, T, U, C, E>,
        sid: SessionId,
        lane: Lane,
    ) -> Result<(), super::error::SpliceError> {
        rendezvous.commit_splice(sid, lane)
    }

    pub(crate) fn release_lane(self, rendezvous: &Rendezvous<'_, '_, T, U, C, E>, lane: Lane) {
        if let Some(sid) = rendezvous.release_lane(lane) {
            rendezvous.emit_lane_release(sid, lane);
        }
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
