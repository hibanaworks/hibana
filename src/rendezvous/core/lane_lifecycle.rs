use super::{
    AssocTable, CapTable, Cell, Clock, Config, ControlOp, ControlScopeKind, EffectContext,
    EffectError, EndpointLeaseId, FREE_REGION_CAPACITY, FreeRegion, GenTable, Generation,
    LabelUniverse, Lane, LaneRelease, LoopTable, PhantomData, PolicyTable, Rendezvous,
    RendezvousId, RouteTable, SessionId, StateSnapshotTable, TapRing, TopologyAck, TopologyError,
    TopologySessionState, TopologyStateTable, Transport, TxAbortError, TxCommitError, Waker, emit,
};
impl<'rv, 'cfg, T: Transport, U: LabelUniverse, C: Clock, E: crate::control::cap::mint::EpochTable>
    Rendezvous<'rv, 'cfg, T, U, C, E>
where
    'cfg: 'rv,
{
    #[cfg(test)]
    fn direct_init_lane_range() -> core::ops::Range<u16> {
        0..0
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
        let runtime_ptr = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { slab.as_mut_ptr().add(runtime_offset) };
        let runtime_len = len - runtime_offset;
        let runtime_slab = /* SAFETY: the pointer and length are carved from one backing slice after bounds and alignment checks. */ unsafe { core::slice::from_raw_parts_mut(runtime_ptr, runtime_len) };
        Some((header_offset as *mut Self, runtime_slab))
    }

    #[cfg(test)]
    unsafe fn init_from_parts(
        dst: *mut Self,
        rv_id: RendezvousId,
        tap_buf: &'cfg mut [crate::observe::core::TapEvent; crate::runtime::consts::RING_EVENTS],
        slab: &mut [u8],
        lane_range: core::ops::Range<u16>,
        clock: C,
        offer_progress_policy: crate::runtime::config::OfferProgressPolicy,
        transport: T,
    ) {
        let image_frontier = 0u32;
        /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
        unsafe {
            core::ptr::addr_of_mut!((*dst).brand_marker).write(PhantomData);
            core::ptr::addr_of_mut!((*dst).id).write(rv_id);
            core::ptr::addr_of_mut!((*dst).tap).write(TapRing::from_storage(tap_buf));
            core::ptr::addr_of_mut!((*dst).slab).write(slab as *mut [u8]);
            core::ptr::addr_of_mut!((*dst).slab_marker).write(PhantomData);
            core::ptr::addr_of_mut!((*dst).image_frontier).write(image_frontier);
            core::ptr::addr_of_mut!((*dst).frontier_workspace_bytes).write(0);
            core::ptr::addr_of_mut!((*dst).endpoint_leases).write(core::ptr::null_mut());
            core::ptr::addr_of_mut!((*dst).endpoint_lease_capacity).write(EndpointLeaseId::ZERO);
            core::ptr::addr_of_mut!((*dst).endpoint_lease_reclaim_delta).write(0);
            core::ptr::addr_of_mut!((*dst).runtime_frontier).write(image_frontier);
            core::ptr::addr_of_mut!((*dst).free_regions)
                .write([FreeRegion::EMPTY; FREE_REGION_CAPACITY]);
            core::ptr::addr_of_mut!((*dst).lane_range)
                .write(u32::from(lane_range.start)..u32::from(lane_range.end));
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
    ) -> Option<*mut Self> {
        let Config {
            tap_buf,
            slab,
            clock,
            offer_progress_policy,
            ..
        } = config;
        let lane_range = Self::direct_init_lane_range();
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
            core::ptr::addr_of_mut!((*dst).clock).write(clock);
            core::ptr::addr_of_mut!((*dst).offer_progress_policy).write(offer_progress_policy);
            core::ptr::addr_of_mut!((*dst)._epoch_marker).write(PhantomData);
        }
        /* SAFETY: the pointer comes from pinned owner storage and this path holds unique mutable access for the borrow. */
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
            core::ptr::addr_of_mut!((*dst).clock).write(clock);
            core::ptr::addr_of_mut!((*dst).offer_progress_policy).write(offer_progress_policy);
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
    ) {
        let Config {
            tap_buf,
            slab,
            clock,
            offer_progress_policy,
            ..
        } = config;
        let lane_range = Self::direct_init_lane_range();
        /* SAFETY: the caller supplies exclusive uninitialized storage and this initializer writes all exposed fields before return. */
        unsafe {
            Self::init_from_parts(
                dst,
                rv_id,
                tap_buf,
                slab,
                lane_range,
                clock,
                offer_progress_policy,
                transport,
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
            ControlScopeKind::Policy | ControlScopeKind::Route | ControlScopeKind::None => {}
        }
    }

    #[inline]
    #[cfg(test)]
    pub(crate) fn state_snapshot_at_lane(&self, sid: SessionId, lane: Lane) -> Generation {
        let epoch = self.lane_generation(lane);
        self.caps.discard_released_lane_entries(lane);
        self.state_snapshots
            .record_snapshot(lane, epoch, self.cap_revision.get());
        self.emit_effect(ControlOp::StateSnapshot, sid, lane, epoch.0 as u32);
        epoch
    }

    #[inline]
    pub(crate) fn publish_prepared_state_snapshot_at_lane(
        &self,
        sid: SessionId,
        lane: Lane,
        generation: Generation,
    ) -> Result<Generation, ()> {
        if self.lane_generation(lane) != generation {
            return Err(());
        }
        self.caps.discard_released_lane_entries(lane);
        self.state_snapshots
            .record_snapshot(lane, generation, self.cap_revision.get());
        self.emit_effect(ControlOp::StateSnapshot, sid, lane, generation.0 as u32);
        Ok(generation)
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
            Err(EffectError::Topology(err)) => {
                let _ = err;
                unreachable!("tx commit effect failure is fully covered")
            }
            Err(EffectError::MissingGeneration)
            | Err(EffectError::Unsupported)
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
            Err(EffectError::Topology(err)) => {
                let _ = err;
                unreachable!("tx abort effect failure is fully covered")
            }
            Err(EffectError::MissingGeneration)
            | Err(EffectError::Unsupported)
            | Err(EffectError::TxCommit(_))
            | Err(EffectError::StateRestore(_)) => {
                unreachable!("tx abort effect failure is fully covered")
            }
        }
    }

    #[inline]
    pub(crate) fn abort_begin_at_lane(&self, sid: SessionId, lane: Lane) {
        self.emit_effect(ControlOp::AbortBegin, sid, lane, lane.as_wire() as u32);
    }

    #[inline]
    pub(crate) fn abort_ack_at_lane(
        &self,
        sid: SessionId,
        lane: Lane,
        generation: Generation,
    ) -> Result<(), ()> {
        if self.lane_generation(lane) != generation {
            return Err(());
        }
        self.emit_effect(ControlOp::AbortAck, sid, lane, generation.0 as u32);
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn is_session_registered(&self, sid: SessionId) -> bool {
        self.assoc.find_lane(sid).is_some()
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

    #[cfg(test)]
    pub(crate) fn advance_lane_generation_to(&self, lane: Lane, target: Generation) {
        if self.r#gen.last(lane).is_none() {
            let _ = self.r#gen.check_and_update(lane, Generation::ZERO);
        }
        if target != Generation::ZERO {
            self.r#gen
                .check_and_update(lane, target)
                .expect("lane generation must advance monotonically");
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
