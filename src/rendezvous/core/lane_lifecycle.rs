use super::{
    AssocTable, CapTable, Cell, Clock, Config, ControlScopeKind, EndpointLeaseId,
    FREE_REGION_CAPACITY, FreeRegion, GenTable, Generation, LabelUniverse, Lane, LaneRelease,
    LoopTable, PhantomData, PolicyTable, Rendezvous, RendezvousId, RouteTable, SessionId,
    StateSnapshotTable, TapRing, TopologyAck, TopologyError, TopologySessionState,
    TopologyStateTable, Transport, Waker, emit,
};

mod prepared_effects;
impl<'rv, 'cfg, T: Transport, U: LabelUniverse, C: Clock, E: crate::control::cap::mint::EpochTable>
    Rendezvous<'rv, 'cfg, T, U, C, E>
where
    'cfg: 'rv,
{
    unsafe fn carve_resident_storage(slab: &mut [u8]) -> Option<(*mut Self, &mut [u8])> {
        let base = slab.as_mut_ptr() as usize;
        let len = slab.len();
        let header_offset = Self::align_up(base, core::mem::align_of::<Self>());
        let header_end = header_offset.checked_add(core::mem::size_of::<Self>())?;
        let runtime_offset = header_end.wrapping_sub(base);
        if runtime_offset > len {
            return None;
        }
        let runtime_ptr = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { slab.as_mut_ptr().add(runtime_offset) };
        let runtime_len = len - runtime_offset;
        let runtime_slab = /* SAFETY: the pointer and length are carved from one backing slice after bounds and alignment checks. */ unsafe { core::slice::from_raw_parts_mut(runtime_ptr, runtime_len) };
        Some((header_offset as *mut Self, runtime_slab))
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
        let Config {
            tap_buf,
            slab,
            clock,
            offer_progress_policy,
            ..
        } = config;
        let lane_range = Config::<U, C>::initial_lane_range();
        let (dst, runtime_slab) = /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */ unsafe { Self::carve_resident_storage(slab) }?;
        let image_frontier = 0u32;
        /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
        unsafe {
            core::ptr::addr_of_mut!((*dst).brand_marker).write(PhantomData);
            core::ptr::addr_of_mut!((*dst).id).write(rv_id);
            core::ptr::addr_of_mut!((*dst).tap).write(TapRing::from_storage(tap_buf));
            core::ptr::addr_of_mut!((*dst).slab).write(runtime_slab as *mut [u8]);
            core::ptr::addr_of_mut!((*dst).slab_marker).write(PhantomData);
            core::ptr::addr_of_mut!((*dst).image_frontier).write(image_frontier);
            core::ptr::addr_of_mut!((*dst).frontier_workspace_bytes).write(0);
            core::ptr::addr_of_mut!((*dst).endpoint_leases).write(core::ptr::null_mut());
            core::ptr::addr_of_mut!((*dst).endpoint_lease_capacity).write(EndpointLeaseId::ZERO);
            core::ptr::addr_of_mut!((*dst).endpoint_lease_reclaim_delta).write(0);
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
            core::ptr::addr_of_mut!((*dst).clock).write(clock);
            core::ptr::addr_of_mut!((*dst).offer_progress_policy).write(offer_progress_policy);
            core::ptr::addr_of_mut!((*dst)._epoch_marker).write(PhantomData);
        }
        Some(dst)
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
            ControlScopeKind::Topology => {
                self.topology.reset_lane(lane);
            }
            ControlScopeKind::Policy | ControlScopeKind::Route | ControlScopeKind::None => {}
        }
    }

    #[inline]
    pub(crate) fn ensure_associated_session_lane(
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

    #[inline]
    pub(crate) fn session_fault(
        &self,
        sid: SessionId,
    ) -> Option<crate::rendezvous::SessionFaultKind> {
        self.assoc.session_fault(sid)
    }

    #[inline]
    pub(crate) fn poison_session(
        &self,
        sid: SessionId,
        cause: crate::rendezvous::SessionFaultKind,
    ) -> crate::rendezvous::SessionFaultKind {
        let kind = self.assoc.poison_session(sid, cause);
        self.assoc.wake_session_waiters(sid);
        self.assoc.for_each_lane(sid, |lane| {
            self.routes.wake_lane_waiters(lane);
        });
        kind
    }

    #[inline]
    pub(crate) fn register_session_waiter(&self, sid: SessionId, lane: Lane, waker: &Waker) {
        self.assoc.register_waiter(sid, lane, waker);
    }

    #[inline]
    pub(crate) fn clear_session_waiter(&self, sid: SessionId, lane: Lane) {
        self.assoc.clear_waiter(sid, lane);
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

    pub(crate) fn restore_lane_runtime_state(&self, lane: Lane, snapshot_cap_revision: u64) {
        self.topology.reset_lane(lane);
        self.caps
            .restore_lane_to_revision(lane, snapshot_cap_revision);
        self.loops.reset_lane(lane);
        self.routes.reset_lane(lane);
    }

    pub(crate) fn reset_lane_recycled_state(&self, lane: Lane) {
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
