//! Endpoint arena initialization.
//!
//! # Unsafe Owner Contract
//!
//! This module owns construction of a `CursorEndpoint` inside a rendezvous
//! lease arena. Unsafe operations write each arena section exactly once using
//! offsets computed by `CursorEndpointStorageLayout`; the caller supplies an
//! exclusive, aligned arena and keeps it resident for the endpoint lifetime.

use crate::endpoint::affine::LaneGuard;
use crate::endpoint::carrier::EndpointOps;
use crate::endpoint::kernel::decision_state::{
    RouteCommitRowSetBuilder, RouteState, RouteStateCapacity, RouteStateStorage,
};
use crate::endpoint::kernel::frontier_state::{
    FrontierState, FrontierStateCapacity, FrontierStateStorage, RootFrontierCapacity,
    RootFrontierStorage,
};
use crate::endpoint::session::SessionCtx;
use crate::global::compiled::images::RoleDescriptorRef;
use crate::global::role_program::DenseLaneOrdinal;
use crate::global::typestate::EventCursor;
use crate::rendezvous::core::EndpointLeaseId;
use crate::rendezvous::port::Port;
use crate::session::brand::Owner;
use crate::session::types::{RendezvousId, SessionId};
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

struct EndpointHeaderInit<'r, const ROLE: u8, T, C, const MAX_RV: usize>
where
    T: Transport + 'r,
    C: crate::runtime_core::config::Clock,
{
    dst: *mut CursorEndpoint<'r, ROLE, T, C, MAX_RV>,
    storage_base: *mut u8,
    storage_layout: CursorEndpointStorageLayout,
    logical_lane_count: usize,
    primary_lane: usize,
    sid: SessionId,
    owner: Owner<'r>,
    public_rv: RendezvousId,
    public_slot: EndpointLeaseId,
    public_generation: u32,
    public_ops: EndpointOps<'r>,
    public_slot_owned: bool,
    session: SessionCtx<'r, T, C, MAX_RV>,
}

#[inline(never)]
unsafe fn init_endpoint_header<'r, const ROLE: u8, T, C, const MAX_RV: usize>(
    init: EndpointHeaderInit<'r, ROLE, T, C, MAX_RV>,
) where
    T: Transport + 'r,
    C: crate::runtime_core::config::Clock,
{
    let EndpointHeaderInit {
        dst,
        storage_base,
        storage_layout,
        logical_lane_count,
        primary_lane,
        sid,
        owner,
        public_rv,
        public_slot,
        public_generation,
        public_ops,
        public_slot_owned,
        session,
    } = init;
    /* SAFETY: the caller supplies exclusive uninitialized storage and this initializer writes all exposed fields before return. */
    unsafe {
        super::lane_slots::LaneSlotArray::init_from_parts(
            ::core::ptr::addr_of_mut!((*dst).ports),
            storage_base
                .add(storage_layout.port_slots_offset())
                .cast::<Option<Port<'r, T>>>(),
            logical_lane_count,
        );
        super::lane_slots::LaneSlotArray::init_from_parts(
            ::core::ptr::addr_of_mut!((*dst).guards),
            storage_base
                .add(storage_layout.guard_slots_offset())
                .cast::<Option<LaneGuard<'r, T, C>>>(),
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
        ::core::ptr::addr_of_mut!((*dst).public_rv).write(public_rv);
        ::core::ptr::addr_of_mut!((*dst).public_slot).write(public_slot);
        ::core::ptr::addr_of_mut!((*dst).public_generation).write(public_generation);
        ::core::ptr::addr_of_mut!((*dst).public_slot_owned).write(public_slot_owned);
        ::core::ptr::addr_of_mut!((*dst).public_active_op).write(super::core::PublicActiveOp::Idle);
        ::core::ptr::addr_of_mut!((*dst).public_offer_state).write(super::offer::OfferState::new());
        ::core::ptr::addr_of_mut!((*dst).public_route_branch).write(None);
        ::core::ptr::addr_of_mut!((*dst).public_recv_state).write(super::recv::RecvState::new());
        ::core::ptr::addr_of_mut!((*dst).public_decode_state)
            .write(super::decode::DecodeState::empty());
        ::core::ptr::addr_of_mut!((*dst).public_send_state).write(super::core::SendState::Done);
        ::core::ptr::addr_of_mut!((*dst).session).write(session);
    }
}

#[inline(never)]
unsafe fn init_endpoint_cursor<'r, const ROLE: u8, T, C, const MAX_RV: usize>(
    dst: *mut CursorEndpoint<'r, ROLE, T, C, MAX_RV>,
    arena_storage: *mut u8,
    arena_layout: &crate::endpoint::kernel::layout::EndpointArenaLayout,
    role_descriptor: RoleDescriptorRef,
) where
    T: Transport + 'r,
    C: crate::runtime_core::config::Clock,
{
    /* SAFETY: the caller supplies exclusive uninitialized storage and this initializer writes all exposed fields before return. */
    unsafe {
        EventCursor::init_from_compiled(
            ::core::ptr::addr_of_mut!((*dst).cursor),
            section_ptr::<crate::global::typestate::EventCursorState>(
                arena_storage,
                arena_layout.event_cursor_state(),
            ),
            section_ptr::<u16>(arena_storage, arena_layout.event_cursor_lane_cursors()),
            section_ptr::<u16>(
                arena_storage,
                arena_layout.event_cursor_current_step_label_codes(),
            ),
            section_ptr::<u32>(
                arena_storage,
                arena_layout.event_cursor_completed_event_words(),
            ),
            role_descriptor,
        );
    }
}

#[inline(never)]
unsafe fn init_endpoint_route<'r, const ROLE: u8, T, C, const MAX_RV: usize>(
    dst: *mut CursorEndpoint<'r, ROLE, T, C, MAX_RV>,
    arena_storage: *mut u8,
    arena_layout: &crate::endpoint::kernel::layout::EndpointArenaLayout,
    role_descriptor: RoleDescriptorRef,
) where
    T: Transport + 'r,
    C: crate::runtime_core::config::Clock,
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
        let route_commit_row_set_builder = section_ptr::<RouteCommitRowSetBuilder>(
            arena_storage,
            arena_layout.route_commit_row_set_builder(),
        );
        LeasedState::init_from_ptr(
            ::core::ptr::addr_of_mut!((*dst).route_commit_rows),
            route_commit_row_set_builder,
        );
        RouteCommitRowSetBuilder::init(
            route_commit_row_set_builder,
            core::ptr::null_mut(),
            role_descriptor.max_route_stack_depth().max(1),
        );
        RouteState::init_empty(
            decision_state,
            RouteStateStorage {
                route_arm_storage: section_ptr::<crate::endpoint::kernel::evidence::RouteArmState>(
                    arena_storage,
                    arena_layout.route_arm_stack(),
                ),
                lane_offer_state_storage: section_ptr::<
                    crate::endpoint::kernel::frontier::LaneOfferState,
                >(
                    arena_storage, arena_layout.lane_offer_state_slots()
                ),
                scope_evidence_slots: section_ptr::<ScopeEvidenceSlot>(
                    arena_storage,
                    arena_layout.scope_evidence_slots(),
                ),
                scope_selected_arms: section_ptr::<
                    crate::endpoint::kernel::decision_state::RouteScopeSelectedArmSlot,
                >(
                    arena_storage,
                    arena_layout.route_state_scope_selected_arms(),
                ),
                lane_dense_by_lane: active_lane_dense_by_lane,
                lane_route_arm_lens: section_ptr::<u8>(
                    arena_storage,
                    arena_layout.route_state_lane_route_arm_lens(),
                ),
                lane_linger_counts: section_ptr::<u8>(
                    arena_storage,
                    arena_layout.route_state_lane_linger_counts(),
                ),
                lane_linger_words: section_ptr::<crate::global::role_program::LaneWord>(
                    arena_storage,
                    arena_layout.route_state_lane_linger_lanes(),
                ),
                lane_offer_linger_words: section_ptr::<crate::global::role_program::LaneWord>(
                    arena_storage,
                    arena_layout.route_state_lane_offer_linger_lanes(),
                ),
                active_offer_lane_words: section_ptr::<crate::global::role_program::LaneWord>(
                    arena_storage,
                    arena_layout.route_state_active_offer_lanes(),
                ),
            },
            RouteStateCapacity {
                lane_slot_count: arena_layout.route_state_lane_dense_by_lane().count(),
                active_lane_count,
                lane_word_count: arena_layout.route_state_lane_linger_lanes().count(),
                lane_offer_state_count: arena_layout.lane_offer_state_slots().count(),
                route_frame_depth: role_descriptor.max_route_stack_depth(),
                scope_evidence_count: arena_layout.scope_evidence_slots().count(),
                scope_selected_arm_count: arena_layout.route_state_scope_selected_arms().count(),
            },
        );
    }
}

#[inline(never)]
unsafe fn init_endpoint_frontier<'r, const ROLE: u8, T, C, const MAX_RV: usize>(
    dst: *mut CursorEndpoint<'r, ROLE, T, C, MAX_RV>,
    arena_storage: *mut u8,
    arena_layout: &crate::endpoint::kernel::layout::EndpointArenaLayout,
    _role_descriptor: RoleDescriptorRef,
) where
    T: Transport + 'r,
    C: crate::runtime_core::config::Clock,
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
            FrontierStateStorage {
                root: RootFrontierStorage {
                    rows: section_ptr::<crate::endpoint::kernel::frontier::RootFrontierState>(
                        arena_storage,
                        arena_layout.frontier_root_rows(),
                    ),
                    active_entries: section_ptr::<crate::endpoint::kernel::frontier::ActiveEntrySlot>(
                        arena_storage,
                        arena_layout.frontier_root_active_slots(),
                    ),
                    observed_key_slots: section_ptr::<
                        crate::endpoint::kernel::frontier::FrontierObservationSlot,
                    >(
                        arena_storage,
                        arena_layout.frontier_root_observed_key_slots(),
                    ),
                    observed_key_offer_lanes: section_ptr::<crate::global::role_program::LaneWord>(
                        arena_storage,
                        arena_layout.frontier_root_observed_offer_lanes(),
                    ),
                },
                offer_entry_slots: section_ptr::<crate::endpoint::kernel::frontier::OfferEntrySlot>(
                    arena_storage,
                    arena_layout.frontier_offer_entry_slots(),
                ),
            },
            FrontierStateCapacity {
                root: RootFrontierCapacity {
                    row_count: arena_layout.frontier_root_rows().count(),
                    pool_capacity: arena_layout.frontier_root_active_slots().count(),
                    observed_key_lane_word_count: arena_layout
                        .frontier_root_observed_offer_lanes()
                        .count()
                        / arena_layout.frontier_root_rows().count().max(1),
                },
                max_offer_entries: arena_layout.frontier_offer_entry_slots().count(),
            },
        );
    }
}

pub(crate) struct CompiledEndpointInit<
    'r,
    const ROLE: u8,
    T: Transport + 'r,
    C: crate::runtime_core::config::Clock,
    const MAX_RV: usize,
> {
    pub dst: *mut CursorEndpoint<'r, ROLE, T, C, MAX_RV>,
    pub arena_storage: *mut u8,
    pub primary_lane: usize,
    pub sid: SessionId,
    pub owner: Owner<'r>,
    pub role_descriptor: RoleDescriptorRef,
    pub public_rv: RendezvousId,
    pub public_slot: EndpointLeaseId,
    pub public_generation: u32,
    pub public_ops: EndpointOps<'r>,
    pub public_slot_owned: bool,
    pub session: SessionCtx<'r, T, C, MAX_RV>,
}

pub(crate) unsafe fn init_empty_from_compiled<'r, const ROLE: u8, T, C, const MAX_RV: usize>(
    init: CompiledEndpointInit<'r, ROLE, T, C, MAX_RV>,
) where
    T: Transport + 'r,
    C: crate::runtime_core::config::Clock,
{
    let CompiledEndpointInit {
        dst,
        arena_storage,
        primary_lane,
        sid,
        owner,
        role_descriptor,
        public_rv,
        public_slot,
        public_generation,
        public_ops,
        public_slot_owned,
        session,
    } = init;
    let arena_layout = role_descriptor.endpoint_arena_layout();
    let lane_slot_count = role_descriptor.endpoint_lane_slot_count();
    let storage_layout =
        super::cursor_endpoint_storage_layout::<ROLE, T, C, MAX_RV>(&arena_layout, lane_slot_count);
    let storage_base = dst.cast::<u8>();
    /* SAFETY: endpoint kernel owns the resident endpoint storage and holds the affine operation borrow for this raw access. */
    unsafe {
        init_endpoint_header(EndpointHeaderInit {
            dst,
            storage_base,
            storage_layout,
            logical_lane_count: lane_slot_count,
            primary_lane,
            sid,
            owner,
            public_rv,
            public_slot,
            public_generation,
            public_ops,
            public_slot_owned,
            session,
        });
        init_endpoint_cursor(dst, arena_storage, &arena_layout, role_descriptor);
        init_endpoint_route(dst, arena_storage, &arena_layout, role_descriptor);
        init_endpoint_frontier(dst, arena_storage, &arena_layout, role_descriptor);
    }
}

pub(crate) unsafe fn write_port_slot<'r, const ROLE: u8, T, C, const MAX_RV: usize>(
    dst: *mut CursorEndpoint<'r, ROLE, T, C, MAX_RV>,
    logical_lane: usize,
    port: Port<'r, T>,
) where
    T: Transport + 'r,
    C: crate::runtime_core::config::Clock,
{
    /* SAFETY: the pointer comes from pinned owner storage and this path holds unique mutable access for the borrow. */
    unsafe {
        (&mut *dst).ports[logical_lane] = Some(port);
    }
}

pub(crate) unsafe fn write_guard_slot<'r, const ROLE: u8, T, C, const MAX_RV: usize>(
    dst: *mut CursorEndpoint<'r, ROLE, T, C, MAX_RV>,
    logical_lane: usize,
    guard: LaneGuard<'r, T, C>,
) where
    T: Transport + 'r,
    C: crate::runtime_core::config::Clock,
{
    /* SAFETY: the pointer comes from pinned owner storage and this path holds unique mutable access for the borrow. */
    unsafe {
        (&mut *dst).guards[logical_lane] = Some(guard);
    }
}

pub(crate) unsafe fn finish_init<'r, const ROLE: u8, T, C, const MAX_RV: usize>(
    dst: *mut CursorEndpoint<'r, ROLE, T, C, MAX_RV>,
) where
    T: Transport + 'r,
    C: crate::runtime_core::config::Clock,
{
    /* SAFETY: the pointer comes from pinned owner storage and this path holds unique mutable access for the borrow. */
    unsafe {
        (&mut *dst).sync_lane_offer_state();
    }
}
