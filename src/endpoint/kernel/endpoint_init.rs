//! Endpoint arena initialization.
//!
//! # Unsafe Owner Contract
//!
//! This module owns construction of a `CursorEndpoint` inside a rendezvous
//! lease arena. Unsafe operations write each arena section exactly once using
//! offsets computed by `CursorEndpointStorageLayout`; the caller supplies an
//! exclusive, aligned arena and keeps it resident for the endpoint lifetime.

use crate::binding::EndpointSlot;
use crate::control::cap::mint::{E0, EndpointEpoch, EpochTable, MintConfigMarker, Owner};
use crate::control::types::{RendezvousId, SessionId};
use crate::endpoint::affine::LaneGuard;
use crate::endpoint::carrier::EndpointOps;
use crate::endpoint::control::SessionControlCtx;
use crate::endpoint::kernel::decision_state::{RouteCommitProofWorkspace, RouteState};
use crate::endpoint::kernel::frontier_state::FrontierState;
use crate::endpoint::kernel::inbox::BindingInbox;
use crate::global::compiled::images::RoleDescriptorRef;
use crate::global::role_program::DenseLaneOrdinal;
use crate::global::typestate::PhaseCursor;
use crate::rendezvous::core::EndpointLeaseId;
use crate::rendezvous::port::Port;
use crate::runtime::consts::LabelUniverse;
use crate::transport::Transport;

use super::CursorEndpoint;
use super::CursorEndpointStorageLayout;
use super::evidence_store::ScopeEvidenceSlot;
use super::layout::EndpointArenaSection;
use super::layout::LeasedState;

#[inline(always)]
unsafe fn section_ptr<T>(base: *mut u8, section: EndpointArenaSection) -> *mut T {
    /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
    unsafe { base.add(section.offset()).cast::<T>() }
}

#[inline(never)]
unsafe fn init_endpoint_header<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>(
    dst: *mut CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
    storage_base: *mut u8,
    storage_layout: CursorEndpointStorageLayout,
    logical_lane_count: usize,
    primary_lane: usize,
    sid: SessionId,
    owner: Owner<'r, E0>,
    epoch: EndpointEpoch<'r, E>,
    public_rv: RendezvousId,
    public_slot: EndpointLeaseId,
    public_generation: u32,
    public_ops: EndpointOps<'r>,
    public_slot_owned: bool,
    offer_progress_policy: crate::runtime::config::OfferProgressPolicy,
    control: SessionControlCtx<'r, T, U, C, E, MAX_RV>,
    mint: Mint,
    binding: B,
) where
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: EndpointSlot + 'r,
{
    /* SAFETY: the caller supplies exclusive uninitialized storage and this initializer writes all exposed fields before return. */
    unsafe {
        super::lane_slots::LaneSlotArray::init_from_parts(
            ::core::ptr::addr_of_mut!((*dst).ports),
            storage_base
                .add(storage_layout.port_slots_offset())
                .cast::<Option<Port<'r, T, E>>>(),
            logical_lane_count,
        );
        super::lane_slots::LaneSlotArray::init_from_parts(
            ::core::ptr::addr_of_mut!((*dst).guards),
            storage_base
                .add(storage_layout.guard_slots_offset())
                .cast::<Option<LaneGuard<'r, T, U, C>>>(),
            logical_lane_count,
        );
        ::core::ptr::addr_of_mut!((*dst).public_header).write(
            crate::endpoint::carrier::KernelEndpointHeader::new(
                public_ops,
                public_generation,
                ROLE,
            ),
        );
        ::core::ptr::addr_of_mut!((*dst).primary_lane).write(primary_lane);
        ::core::ptr::addr_of_mut!((*dst).sid).write(sid);
        ::core::ptr::addr_of_mut!((*dst)._owner).write(owner);
        ::core::ptr::addr_of_mut!((*dst)._epoch).write(epoch);
        ::core::ptr::addr_of_mut!((*dst).public_rv).write(public_rv);
        ::core::ptr::addr_of_mut!((*dst).public_slot).write(public_slot);
        ::core::ptr::addr_of_mut!((*dst).public_generation).write(public_generation);
        ::core::ptr::addr_of_mut!((*dst).public_slot_owned).write(public_slot_owned);
        ::core::ptr::addr_of_mut!((*dst).public_offer_state).write(super::offer::OfferState::new());
        ::core::ptr::addr_of_mut!((*dst).public_route_branch).write(None);
        ::core::ptr::addr_of_mut!((*dst).public_recv_state).write(super::recv::RecvState::new());
        ::core::ptr::addr_of_mut!((*dst).public_decode_state)
            .write(super::decode::DecodeState::empty());
        ::core::ptr::addr_of_mut!((*dst).public_send_state).write(super::core::SendState::Done);
        ::core::ptr::addr_of_mut!((*dst).offer_progress_policy).write(offer_progress_policy);
        ::core::ptr::addr_of_mut!((*dst).control).write(control);
        ::core::ptr::addr_of_mut!((*dst).mint).write(mint.as_config());
        ::core::ptr::addr_of_mut!((*dst).restored_binding_payload).write(None);
        ::core::ptr::addr_of_mut!((*dst).binding).write(binding);
    }
}

#[inline(never)]
unsafe fn init_endpoint_cursor<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>(
    dst: *mut CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
    arena_storage: *mut u8,
    arena_layout: &crate::endpoint::kernel::layout::EndpointArenaLayout,
    role_descriptor: RoleDescriptorRef,
) where
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: EndpointSlot,
{
    /* SAFETY: the caller supplies exclusive uninitialized storage and this initializer writes all exposed fields before return. */
    unsafe {
        PhaseCursor::init_from_compiled(
            ::core::ptr::addr_of_mut!((*dst).cursor),
            section_ptr::<crate::global::typestate::PhaseCursorState>(
                arena_storage,
                arena_layout.phase_cursor_state(),
            ),
            section_ptr::<u16>(arena_storage, arena_layout.phase_cursor_lane_cursors()),
            section_ptr::<u16>(
                arena_storage,
                arena_layout.phase_cursor_current_step_label_codes(),
            ),
            role_descriptor,
        );
    }
}

#[inline(never)]
unsafe fn init_endpoint_route<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>(
    dst: *mut CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
    arena_storage: *mut u8,
    arena_layout: &crate::endpoint::kernel::layout::EndpointArenaLayout,
    role_descriptor: RoleDescriptorRef,
) where
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: EndpointSlot,
{
    /* SAFETY: endpoint kernel owns the resident endpoint storage and holds the affine operation borrow for this raw access. */
    unsafe {
        let active_lane_dense_by_lane = section_ptr::<DenseLaneOrdinal>(
            arena_storage,
            arena_layout.route_state_lane_dense_by_lane(),
        );
        let active_lane_count =
            role_descriptor.fill_active_lane_dense_by_lane(core::slice::from_raw_parts_mut(
                active_lane_dense_by_lane,
                arena_layout.route_state_lane_dense_by_lane().count(),
            ));
        let decision_state =
            section_ptr::<RouteState>(arena_storage, arena_layout.decision_state());
        LeasedState::init_from_ptr(
            ::core::ptr::addr_of_mut!((*dst).decision_state),
            decision_state,
        );
        let route_commit_proof_workspace = section_ptr::<RouteCommitProofWorkspace>(
            arena_storage,
            arena_layout.route_commit_proof_workspace(),
        );
        LeasedState::init_from_ptr(
            ::core::ptr::addr_of_mut!((*dst).route_commit_proofs),
            route_commit_proof_workspace,
        );
        RouteCommitProofWorkspace::init(
            route_commit_proof_workspace,
            section_ptr::<crate::endpoint::kernel::decision_state::RouteArmCommitProof>(
                arena_storage,
                arena_layout.route_state_commit_proofs(),
            ),
            arena_layout.route_state_commit_proofs().count(),
        );
        RouteState::init_empty(
            decision_state,
            section_ptr::<crate::endpoint::kernel::evidence::RouteArmState>(
                arena_storage,
                arena_layout.route_arm_stack(),
            ),
            section_ptr::<crate::endpoint::kernel::frontier::LaneOfferState>(
                arena_storage,
                arena_layout.lane_offer_state_slots(),
            ),
            section_ptr::<ScopeEvidenceSlot>(arena_storage, arena_layout.scope_evidence_slots()),
            section_ptr::<crate::endpoint::kernel::decision_state::RouteScopeSelectedArmSlot>(
                arena_storage,
                arena_layout.route_state_scope_selected_arms(),
            ),
            active_lane_dense_by_lane,
            arena_layout.route_state_lane_dense_by_lane().count(),
            section_ptr::<u8>(
                arena_storage,
                arena_layout.route_state_lane_route_arm_lens(),
            ),
            section_ptr::<u8>(arena_storage, arena_layout.route_state_lane_linger_counts()),
            section_ptr::<crate::global::role_program::LaneWord>(
                arena_storage,
                arena_layout.route_state_active_route_lanes(),
            ),
            section_ptr::<crate::global::role_program::LaneWord>(
                arena_storage,
                arena_layout.route_state_lane_linger_lanes(),
            ),
            section_ptr::<crate::global::role_program::LaneWord>(
                arena_storage,
                arena_layout.route_state_lane_offer_linger_lanes(),
            ),
            section_ptr::<crate::global::role_program::LaneWord>(
                arena_storage,
                arena_layout.route_state_active_offer_lanes(),
            ),
            active_lane_count,
            arena_layout.route_state_active_route_lanes().count(),
            arena_layout.lane_offer_state_slots().count(),
            role_descriptor.max_route_stack_depth(),
            arena_layout.scope_evidence_slots().count(),
            arena_layout.route_state_scope_selected_arms().count(),
        );
    }
}

#[inline(never)]
unsafe fn init_endpoint_frontier<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>(
    dst: *mut CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
    arena_storage: *mut u8,
    arena_layout: &crate::endpoint::kernel::layout::EndpointArenaLayout,
    _role_descriptor: RoleDescriptorRef,
) where
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: EndpointSlot,
{
    /* SAFETY: the caller supplies exclusive uninitialized storage and this initializer writes all exposed fields before return. */
    unsafe {
        let frontier_state =
            section_ptr::<FrontierState>(arena_storage, arena_layout.frontier_state());
        LeasedState::init_from_ptr(
            ::core::ptr::addr_of_mut!((*dst).frontier_state),
            frontier_state,
        );
        FrontierState::init_empty(
            frontier_state,
            section_ptr::<crate::endpoint::kernel::frontier::RootFrontierState>(
                arena_storage,
                arena_layout.frontier_root_rows(),
            ),
            section_ptr::<crate::endpoint::kernel::frontier::ActiveEntrySlot>(
                arena_storage,
                arena_layout.frontier_root_active_slots(),
            ),
            section_ptr::<crate::endpoint::kernel::frontier::FrontierObservationSlot>(
                arena_storage,
                arena_layout.frontier_root_observed_key_slots(),
            ),
            section_ptr::<crate::global::role_program::LaneWord>(
                arena_storage,
                arena_layout.frontier_root_observed_offer_lanes(),
            ),
            section_ptr::<crate::global::role_program::LaneWord>(
                arena_storage,
                arena_layout.frontier_root_observed_binding_nonempty_lanes(),
            ),
            section_ptr::<crate::endpoint::kernel::frontier::OfferEntrySlot>(
                arena_storage,
                arena_layout.frontier_offer_entry_slots(),
            ),
            arena_layout.frontier_root_rows().count(),
            arena_layout.frontier_root_active_slots().count(),
            arena_layout.frontier_root_observed_offer_lanes().count()
                / arena_layout.frontier_root_rows().count().max(1),
            arena_layout.frontier_offer_entry_slots().count(),
        );
    }
}

#[inline(never)]
unsafe fn init_endpoint_binding<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>(
    dst: *mut CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
    arena_storage: *mut u8,
    arena_layout: &crate::endpoint::kernel::layout::EndpointArenaLayout,
    role_descriptor: RoleDescriptorRef,
    binding_enabled: bool,
) where
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: EndpointSlot,
{
    /* SAFETY: endpoint kernel owns the resident endpoint storage and holds the affine operation borrow for this raw access. */
    unsafe {
        let logical_lane_dense_by_lane = section_ptr::<DenseLaneOrdinal>(
            arena_storage,
            arena_layout.binding_lane_dense_by_lane(),
        );
        let logical_lane_count = if binding_enabled {
            role_descriptor.fill_logical_lane_dense_by_lane(core::slice::from_raw_parts_mut(
                logical_lane_dense_by_lane,
                arena_layout.binding_lane_dense_by_lane().count(),
            ))
        } else {
            0
        };
        let binding_inbox =
            section_ptr::<BindingInbox>(arena_storage, arena_layout.binding_inbox());
        LeasedState::init_from_ptr(
            ::core::ptr::addr_of_mut!((*dst).binding_inbox),
            binding_inbox,
        );
        BindingInbox::init_empty(
            binding_inbox,
            section_ptr::<crate::endpoint::kernel::inbox::PackedIngressEvidence>(
                arena_storage,
                arena_layout.binding_slots(),
            ),
            section_ptr::<u8>(arena_storage, arena_layout.binding_len()),
            section_ptr::<crate::transport::FrameLabelMask>(
                arena_storage,
                arena_layout.binding_frame_label_masks(),
            ),
            section_ptr::<crate::global::role_program::LaneWord>(
                arena_storage,
                arena_layout.binding_nonempty_lanes(),
            ),
            logical_lane_dense_by_lane,
            logical_lane_count,
            arena_layout.binding_nonempty_lanes().count(),
        );
    }
}

pub(crate) struct CompiledEndpointInit<
    'r,
    const ROLE: u8,
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    const MAX_RV: usize,
    Mint: MintConfigMarker,
    B: EndpointSlot + 'r,
> {
    pub dst: *mut CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
    pub arena_storage: *mut u8,
    pub primary_lane: usize,
    pub sid: SessionId,
    pub owner: Owner<'r, E0>,
    pub epoch: EndpointEpoch<'r, E>,
    pub role_descriptor: RoleDescriptorRef,
    pub public_rv: RendezvousId,
    pub public_slot: EndpointLeaseId,
    pub public_generation: u32,
    pub public_ops: EndpointOps<'r>,
    pub public_slot_owned: bool,
    pub offer_progress_policy: crate::runtime::config::OfferProgressPolicy,
    pub control: SessionControlCtx<'r, T, U, C, E, MAX_RV>,
    pub mint: Mint,
    pub binding_enabled: bool,
    pub binding: B,
}

pub(crate) unsafe fn init_empty_from_compiled<
    'r,
    const ROLE: u8,
    T,
    U,
    C,
    E,
    const MAX_RV: usize,
    Mint,
    B,
>(
    init: CompiledEndpointInit<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
) where
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: EndpointSlot + 'r,
{
    let CompiledEndpointInit {
        dst,
        arena_storage,
        primary_lane,
        sid,
        owner,
        epoch,
        role_descriptor,
        public_rv,
        public_slot,
        public_generation,
        public_ops,
        public_slot_owned,
        offer_progress_policy,
        control,
        mint,
        binding_enabled,
        binding,
    } = init;
    let arena_layout = role_descriptor.endpoint_arena_layout_for_binding(binding_enabled);
    let lane_slot_count = role_descriptor.endpoint_lane_slot_count();
    let storage_layout = super::cursor_endpoint_storage_layout::<ROLE, T, U, C, E, MAX_RV, Mint, B>(
        &arena_layout,
        lane_slot_count,
    );
    let storage_base = dst.cast::<u8>();
    /* SAFETY: endpoint kernel owns the resident endpoint storage and holds the affine operation borrow for this raw access. */
    unsafe {
        init_endpoint_header(
            dst,
            storage_base,
            storage_layout,
            lane_slot_count,
            primary_lane,
            sid,
            owner,
            epoch,
            public_rv,
            public_slot,
            public_generation,
            public_ops,
            public_slot_owned,
            offer_progress_policy,
            control,
            mint,
            binding,
        );
        init_endpoint_cursor(dst, arena_storage, &arena_layout, role_descriptor);
        init_endpoint_route(dst, arena_storage, &arena_layout, role_descriptor);
        init_endpoint_frontier(dst, arena_storage, &arena_layout, role_descriptor);
        init_endpoint_binding(
            dst,
            arena_storage,
            &arena_layout,
            role_descriptor,
            binding_enabled,
        );
    }
}

pub(crate) unsafe fn write_port_slot<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>(
    dst: *mut CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
    logical_lane: usize,
    port: Port<'r, T, E>,
) where
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: EndpointSlot,
{
    /* SAFETY: the pointer comes from pinned owner storage and this path holds unique mutable access for the borrow. */
    unsafe {
        (&mut *dst).ports[logical_lane] = Some(port);
    }
}

pub(crate) unsafe fn write_guard_slot<
    'r,
    const ROLE: u8,
    T,
    U,
    C,
    E,
    const MAX_RV: usize,
    Mint,
    B,
>(
    dst: *mut CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
    logical_lane: usize,
    guard: LaneGuard<'r, T, U, C>,
) where
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: EndpointSlot,
{
    /* SAFETY: the pointer comes from pinned owner storage and this path holds unique mutable access for the borrow. */
    unsafe {
        (&mut *dst).guards[logical_lane] = Some(guard);
    }
}

pub(crate) unsafe fn finish_init<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>(
    dst: *mut CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
) where
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: EndpointSlot,
{
    /* SAFETY: the pointer comes from pinned owner storage and this path holds unique mutable access for the borrow. */
    unsafe {
        (&mut *dst).sync_lane_offer_state();
    }
}
