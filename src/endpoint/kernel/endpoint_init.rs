//! Endpoint arena initialization.
//!
//! # Unsafe Owner Contract
//!
//! This module owns construction of a `ResidentActor` inside a rendezvous
//! lease arena. Unsafe operations write each arena section exactly once using
//! offsets computed by `ActorStorageLayout`; the caller supplies an
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
use crate::session::types::{RendezvousId, SessionId};
use crate::transport::Transport;

use super::ActorStorageLayout;
use super::ResidentActor;
use super::core::{ActorProgress, ActorViews, EndpointIo, ResidentOperation, ResidentView};
use super::evidence_store::ScopeEvidenceSlot;
use super::layout::EndpointArenaSection;
use super::layout::LeasedState;

/// Recover the immutable compiled-program authority already resident in a
/// published endpoint without adding another pointer to the lease ledger.
///
/// # Safety
///
/// `storage..storage + storage_len` must contain one fully initialized
/// `ResidentActor<'r, T>` allocation. `ResidentActor` is `repr(C)`;
/// its logical role is installed from the immutable projected descriptor.
pub(crate) unsafe fn resident_program_ref<'r, T>(
    storage: *const u8,
    storage_len: usize,
) -> &'static crate::global::compiled::images::CompiledProgramRef
where
    T: Transport + 'r,
{
    let program_offset = crate::invariant_some(
        core::mem::offset_of!(ResidentActor<'r, T>, context).checked_add(core::mem::offset_of!(
            super::core::ActorContext<'r, T>,
            program
        )),
    );
    let required = crate::invariant_some(program_offset.checked_add(core::mem::size_of::<
        &'static crate::global::compiled::images::CompiledProgramRef,
    >()));
    if required > storage_len {
        crate::invariant();
    }
    let program = /* SAFETY: the caller provides a live initialized endpoint;
    `program_offset` is the role-independent field offset and the
    complete field range was checked above. */ unsafe {
        storage.add(program_offset).cast::<&'static crate::global::compiled::images::CompiledProgramRef>()
    };
    if !(program as usize).is_multiple_of(core::mem::align_of::<
        &'static crate::global::compiled::images::CompiledProgramRef,
    >()) {
        crate::invariant();
    }
    /* SAFETY: this immutable pointer is outside the progress borrow cell;
    the compiled rows outlive the published actor allocation. */
    unsafe { *program }
}

#[inline(always)]
unsafe fn section_ptr<T>(base: *mut u8, section: EndpointArenaSection) -> *mut T {
    /* SAFETY: `ActorStorageLayout` produced `section` for the endpoint
    arena passed by the rendezvous lease. The section offset/alignment/size were
    checked when the arena layout was built. */
    unsafe { base.add(section.offset()).cast::<T>() }
}

struct EndpointHeaderInit<'r, T>
where
    T: Transport + 'r,
{
    dst: *mut ResidentActor<'r, T>,
    storage_base: *mut u8,
    storage_layout: ActorStorageLayout,
    descriptor: RoleDescriptorRef,
    initial_role: u8,
    primary_lane: usize,
    sid: SessionId,
    public_rv: RendezvousId,
    public_slot: EndpointLeaseId,
    public_generation: u32,
    public_ops: EndpointOps,
    public_slot_ownership: super::core::PublicSlotOwnership,
    session: SessionCtx<'r, T>,
}

#[inline(never)]
unsafe fn init_endpoint_header<'r, T>(init: EndpointHeaderInit<'r, T>)
where
    T: Transport + 'r,
{
    let EndpointHeaderInit {
        dst,
        storage_base,
        storage_layout,
        descriptor,
        initial_role,
        primary_lane,
        sid,
        public_rv,
        public_slot,
        public_generation,
        public_ops,
        public_slot_ownership,
        session,
    } = init;
    if session.rendezvous().registry_id() != public_rv {
        crate::invariant();
    }
    /* SAFETY: `init_empty_from_compiled` passes the unpublished
    `ResidentActor` header field set and the endpoint arena base. This block
    initializes public state, lane slots, and session context before returning
    the endpoint to the rendezvous lease. */
    unsafe {
        let context = ::core::ptr::addr_of_mut!((*dst).context);
        ::core::ptr::addr_of_mut!((*dst).progress)
            .write(core::cell::RefCell::new(core::mem::MaybeUninit::uninit()));
        ::core::ptr::addr_of_mut!((*dst).public_header)
            .write(crate::endpoint::carrier::KernelActorHeader::new(public_ops));
        let actor_header = crate::invariant_some(core::ptr::NonNull::new(
            ::core::ptr::addr_of_mut!((*dst).public_header),
        ));
        let views = descriptor.logical_views();
        let view_ptr = storage_base
            .add(storage_layout.views_offset())
            .cast::<ResidentView<'r, T>>();
        ::core::ptr::addr_of_mut!((*dst).views)
            .write(ActorViews::from_parts(view_ptr, views.len()));
        let mut lane_offset = 0;
        for (ordinal, role) in views.roles().enumerate() {
            let view = view_ptr.add(ordinal);
            let io = ::core::ptr::addr_of_mut!((*view).io).cast::<EndpointIo<'r, T>>();
            let logical_lane_count = descriptor.view_lane_slot_count(role);
            super::lane_slots::LaneSlotArray::init_from_parts(
                ::core::ptr::addr_of_mut!((*io).ports),
                storage_base
                    .add(storage_layout.port_slots_offset())
                    .cast::<Option<Port<'r, T>>>()
                    .add(lane_offset),
                logical_lane_count,
            );
            super::lane_slots::LaneSlotArray::init_from_parts(
                ::core::ptr::addr_of_mut!((*io).guards),
                storage_base
                    .add(storage_layout.guard_slots_offset())
                    .cast::<Option<LaneGuard<'r, T>>>()
                    .add(lane_offset),
                logical_lane_count,
            );
            super::lane_slots::LaneSlotArray::init_from_parts(
                ::core::ptr::addr_of_mut!((*view).operations),
                storage_base
                    .add(storage_layout.operation_slots_offset())
                    .cast::<Option<ResidentOperation<'r>>>()
                    .add(lane_offset),
                logical_lane_count,
            );
            for operation in 0..logical_lane_count {
                *crate::invariant_some((*view).operations.get_mut(operation)) =
                    Some(ResidentOperation::new());
            }
            ::core::ptr::addr_of_mut!((*view).public_header).write(
                crate::endpoint::carrier::KernelEndpointHeader::new(
                    actor_header,
                    public_generation,
                    role,
                ),
            );
            ::core::ptr::addr_of_mut!((*view).retired_next).write(core::cell::Cell::new(None));
            let view_primary = if role == initial_role {
                primary_lane
            } else {
                0
            };
            if view_primary >= logical_lane_count {
                crate::invariant();
            }
            ::core::ptr::addr_of_mut!((*io).primary_lane).write(view_primary);
            if role == initial_role && !(*view).public_header.issue_initial(1) {
                crate::invariant();
            }
            lane_offset += logical_lane_count;
        }
        ::core::ptr::addr_of_mut!((*context).sid).write(sid);
        ::core::ptr::addr_of_mut!((*context).program).write(descriptor.program());
        ::core::ptr::addr_of_mut!((*context).public_rv).write(public_rv);
        ::core::ptr::addr_of_mut!((*context).public_slot).write(public_slot);
        ::core::ptr::addr_of_mut!((*context).public_generation)
            .write(core::cell::Cell::new(public_generation));
        ::core::ptr::addr_of_mut!((*context).view_generation).write(core::cell::Cell::new(1));
        ::core::ptr::addr_of_mut!((*context).live_views).write(core::cell::Cell::new(1));
        ::core::ptr::addr_of_mut!((*context).retired_views).write(core::cell::Cell::new(None));
        ::core::ptr::addr_of_mut!((*context).retired_operations).write(core::cell::Cell::new(None));
        ::core::ptr::addr_of_mut!((*context).public_slot_ownership)
            .write(core::cell::Cell::new(public_slot_ownership));
        ::core::ptr::addr_of_mut!((*context).session).write(session);
    }
}

#[inline(never)]
unsafe fn init_endpoint_cursor<'r, T>(
    dst: *mut ResidentActor<'r, T>,
    arena_storage: *mut u8,
    arena_layout: &crate::endpoint::kernel::layout::EndpointArenaLayout,
    role_descriptor: RoleDescriptorRef,
) where
    T: Transport + 'r,
{
    /* SAFETY: the actor cursor is still unpublished, and all cursor backing
    columns come from the endpoint arena sections selected by the compiled role
    descriptor. `EventCursor::init_from_compiled` initializes the cursor before
    endpoint publication. */
    unsafe {
        let progress = (*dst).progress.as_ptr().cast::<ActorProgress>();
        EventCursor::init_from_compiled(
            ::core::ptr::addr_of_mut!((*progress).cursor),
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
unsafe fn init_endpoint_route<'r, T>(
    dst: *mut ResidentActor<'r, T>,
    arena_storage: *mut u8,
    arena_layout: &crate::endpoint::kernel::layout::EndpointArenaLayout,
    role_descriptor: RoleDescriptorRef,
) where
    T: Transport + 'r,
{
    /* SAFETY: route-state initialization owns the unpublished endpoint route
    fields. Every pointer passed to `RouteState::init_empty` is a section of the
    endpoint arena, and active-lane density is filled before route state reads it. */
    unsafe {
        let progress = (*dst).progress.as_ptr().cast::<ActorProgress>();
        let active_lane_dense_by_lane = section_ptr::<DenseLaneOrdinal>(
            arena_storage,
            arena_layout.route_state_lane_dense_by_lane(),
        );
        let active_lane_count =
            role_descriptor.fill_active_lane_dense_by_lane(core::slice::from_raw_parts_mut(
                active_lane_dense_by_lane,
                arena_layout.route_state_lane_dense_by_lane().count(),
            ));
        if active_lane_count != arena_layout.lane_offer_state_slots().count() {
            crate::invariant();
        }
        let decision_state =
            section_ptr::<RouteState>(arena_storage, arena_layout.decision_state());
        LeasedState::init_from_ptr(
            ::core::ptr::addr_of_mut!((*progress).decision_state),
            decision_state,
        );
        let route_commit_row_set_builder = section_ptr::<RouteCommitRowSetBuilder>(
            arena_storage,
            arena_layout.route_commit_row_set_builder(),
        );
        LeasedState::init_from_ptr(
            ::core::ptr::addr_of_mut!((*progress).route_commit_rows),
            route_commit_row_set_builder,
        );
        RouteCommitRowSetBuilder::init(
            route_commit_row_set_builder,
            role_descriptor.max_route_commit_count(),
        );
        RouteState::init_empty(
            decision_state,
            RouteStateStorage {
                route_arm_storage: section_ptr::<crate::endpoint::kernel::evidence::RouteArmState>(
                    arena_storage,
                    arena_layout.route_arm_history(),
                ),
                lane_route_arm_lengths: section_ptr::<u16>(
                    arena_storage,
                    arena_layout.route_state_lane_route_arm_lengths(),
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
                lane_reentry_words: section_ptr::<crate::global::role_program::LaneWord>(
                    arena_storage,
                    arena_layout.route_state_lane_reentry_lanes(),
                ),
                lane_offer_reentry_words: section_ptr::<crate::global::role_program::LaneWord>(
                    arena_storage,
                    arena_layout.route_state_lane_offer_reentry_lanes(),
                ),
                active_offer_lane_words: section_ptr::<crate::global::role_program::LaneWord>(
                    arena_storage,
                    arena_layout.route_state_active_offer_lanes(),
                ),
            },
            RouteStateCapacity {
                lane_slot_count: arena_layout.route_state_lane_dense_by_lane().count(),
                lane_word_count: arena_layout.route_state_lane_reentry_lanes().count(),
                lane_offer_state_count: arena_layout.lane_offer_state_slots().count(),
                route_arm_state_capacity: arena_layout.route_arm_history().count(),
                scope_evidence_count: arena_layout.scope_evidence_slots().count(),
                scope_selected_arm_count: arena_layout.route_state_scope_selected_arms().count(),
            },
        );
    }
}

#[inline(never)]
unsafe fn init_endpoint_frontier<'r, T>(
    dst: *mut ResidentActor<'r, T>,
    arena_storage: *mut u8,
    arena_layout: &crate::endpoint::kernel::layout::EndpointArenaLayout,
    role_descriptor: RoleDescriptorRef,
) where
    T: Transport + 'r,
{
    /* SAFETY: frontier initialization owns the unpublished endpoint frontier
    field. The root rows, active slots, observation slots, and offer-entry slots
    are disjoint arena sections selected by the compiled endpoint layout. */
    unsafe {
        let progress = (*dst).progress.as_ptr().cast::<ActorProgress>();
        let frontier_state =
            section_ptr::<FrontierState>(arena_storage, arena_layout.frontier_state());
        LeasedState::init_from_ptr(
            ::core::ptr::addr_of_mut!((*progress).frontier_state),
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
                },
                visited_position_bits: section_ptr::<u8>(
                    arena_storage,
                    arena_layout.frontier_visited_position_bits(),
                ),
            },
            FrontierStateCapacity {
                root: RootFrontierCapacity {
                    row_count: arena_layout.frontier_root_rows().count(),
                    pool_capacity: arena_layout.frontier_root_active_slots().count(),
                },
                visited_position_count: role_descriptor
                    .local_event_rows()
                    .footprint()
                    .frontier_visit_position_count(),
            },
        );
    }
}

pub(crate) struct CompiledEndpointInit<'r, T: Transport + 'r> {
    pub(crate) dst: *mut ResidentActor<'r, T>,
    pub(crate) arena_storage: *mut u8,
    pub(crate) primary_lane: usize,
    pub(crate) initial_role: u8,
    pub(crate) sid: SessionId,
    pub(crate) role_descriptor: RoleDescriptorRef,
    pub(crate) public_rv: RendezvousId,
    pub(crate) public_slot: EndpointLeaseId,
    pub(crate) public_generation: u32,
    pub(crate) public_ops: EndpointOps,
    pub(crate) public_slot_ownership: super::core::PublicSlotOwnership,
    pub(crate) session: SessionCtx<'r, T>,
}

pub(crate) unsafe fn init_empty_from_compiled<'r, T>(init: CompiledEndpointInit<'r, T>)
where
    T: Transport + 'r,
{
    let CompiledEndpointInit {
        dst,
        arena_storage,
        primary_lane,
        initial_role,
        sid,
        role_descriptor,
        public_rv,
        public_slot,
        public_generation,
        public_ops,
        public_slot_ownership,
        session,
    } = init;
    let arena_layout = role_descriptor.endpoint_arena_layout();
    let storage_layout = super::actor_storage_layout::<T>(role_descriptor);
    let storage_base = dst.cast::<u8>();
    /* SAFETY: the rendezvous lease passes the unpublished endpoint allocation.
    Header, cursor, route, and frontier owners initialize disjoint fields and
    arena sections before the endpoint is made reachable. */
    unsafe {
        init_endpoint_header(EndpointHeaderInit {
            dst,
            storage_base,
            storage_layout,
            descriptor: role_descriptor,
            initial_role,
            primary_lane,
            sid,
            public_rv,
            public_slot,
            public_generation,
            public_ops,
            public_slot_ownership,
            session,
        });
        init_endpoint_cursor(dst, arena_storage, &arena_layout, role_descriptor);
        init_endpoint_route(dst, arena_storage, &arena_layout, role_descriptor);
        init_endpoint_frontier(dst, arena_storage, &arena_layout, role_descriptor);
    }
}

pub(crate) unsafe fn write_port_slot<'r, T>(
    dst: *mut ResidentActor<'r, T>,
    role: u8,
    logical_lane: usize,
    port: Port<'r, T>,
) where
    T: Transport + 'r,
{
    /* SAFETY: endpoint attachment owns `dst` while installing lane resources.
    `logical_lane` indexes the initialized port-slot array, and the write occurs
    before a public endpoint borrow can access that lane. */
    unsafe {
        let view = crate::invariant_some((*dst).views.get(role));
        (&mut *view.io.get()).ports[logical_lane] = Some(port);
    }
}

pub(crate) unsafe fn write_guard_slot<'r, T>(
    dst: *mut ResidentActor<'r, T>,
    role: u8,
    logical_lane: usize,
    guard: LaneGuard<'r, T>,
) where
    T: Transport + 'r,
{
    /* SAFETY: endpoint attachment owns `dst` while installing lane resources.
    `logical_lane` indexes the initialized guard-slot array, and this consumes
    the guard into that one slot. */
    unsafe {
        let view = crate::invariant_some((*dst).views.get(role));
        (&mut *view.io.get()).guards[logical_lane] = Some(guard);
    }
}

pub(crate) unsafe fn finish_init<'r, T>(dst: *mut ResidentActor<'r, T>, role: u8)
where
    T: Transport + 'r,
{
    /* SAFETY: endpoint initialization still owns `dst`; lane offer state is
    synchronized after all lane slots and route/frontier sections have been
    initialized and before endpoint publication. */
    unsafe {
        let actor = &*dst;
        crate::invariant_some(actor.try_with_kernel(
            crate::invariant_some(actor.views.get(role)),
            crate::endpoint::carrier::OperationRegion::WHOLE,
            |kernel| kernel.sync_lane_offer_state(),
        ));
    }
}
