use super::{
    CapTable, Cell, Clock, EpochPortGuard, Guard, LabelUniverse, Lane, LaneGuard, PhantomData,
    Port, PortInit, RawEvent, Rendezvous, RendezvousError, SessionId, TapRing, Transport, emit,
};
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
    pub(crate) fn offer_progress_policy(&self) -> crate::runtime::config::OfferProgressPolicy {
        self.offer_progress_policy
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
        if !self.lane_range.contains(&lane.raw()) {
            return Err(RendezvousError::LaneOutOfRange { lane });
        }
        if self.session_fault(sid).is_some() {
            return Err(RendezvousError::SessionPoisoned { sid });
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
            Some(_other) => {
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
                .with_arg1(lane.raw()),
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

    pub(crate) fn open_port_guard<'a>(
        &'a self,
        sid: SessionId,
        lane: Lane,
        role: u8,
        role_count: u8,
        active_leases: &'a Cell<u32>,
    ) -> Result<EpochPortGuard<'a, T, U, C>, RendezvousError>
    where
        'rv: 'a,
    {
        let (tx, rx) = self
            .transport
            .open(crate::transport::PortOpen::from_descriptor(role, sid, lane));
        let port = Port::new(PortInit {
            transport: &self.transport,
            tap: self.tap(),
            clock: &self.clock,
            loops: &self.loops,
            routes: &self.routes,
            slab: self.slab,
            image_frontier: core::ptr::addr_of!(self.image_frontier),
            frontier_workspace_bytes: core::ptr::addr_of!(self.frontier_workspace_bytes),
            endpoint_leases: self.endpoint_leases.cast_const(),
            endpoint_lease_capacity: self.endpoint_lease_capacity,
            lane,
            role,
            role_count,
            rv_id: self.id,
            tx,
            rx,
            _epoch: PhantomData,
        });
        let guard =
            LaneGuard::new_detached((self as *const Self).cast::<()>(), lane, active_leases);
        Ok((port, guard))
    }

    // ============================================================================
    // Capability methods
    // ============================================================================
}
