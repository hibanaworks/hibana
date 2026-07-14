use super::{
    Cell, Guard, Lane, LaneGuard, Port, PortInit, Rendezvous, RendezvousError, SessionId, TapRing,
    Transport, emit, events,
};
impl<'rv, 'cfg, T: Transport> Rendezvous<'rv, 'cfg, T>
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

    pub(crate) fn now32(&self) -> u32 {
        let current = self.tap_counter.get();
        if current != u32::MAX {
            self.tap_counter.set(current + 1);
        }
        current
    }

    pub(crate) fn activate_lane_attachment(
        &self,
        sid: SessionId,
        lane: Lane,
    ) -> Result<(), RendezvousError> {
        if lane.raw() < self.lane_base.get() || lane.raw() >= self.lane_end.get() {
            return Err(RendezvousError::LaneOutOfRange { lane });
        }
        if self.session_fault(sid).is_some() {
            return Err(RendezvousError::SessionPoisoned { sid });
        }
        let first_attach = if self.assoc.has_entry(lane, sid) {
            if self.assoc.increment(lane, sid).is_none() {
                return Err(RendezvousError::LaneAttachOverflow { lane });
            }
            false
        } else {
            if !self.assoc.register(lane, sid) {
                return Err(RendezvousError::LaneAttachOverflow { lane });
            }
            true
        };

        if first_attach {
            emit(
                self.tap(),
                events::lane_acquire(
                    self.now32(),
                    self.id.raw() as u32,
                    sid.raw(),
                    lane.as_wire() as u16,
                ),
            );
        }
        Ok(())
    }

    #[inline]
    pub(crate) fn has_lane_attachment(&self, sid: SessionId, lane: Lane) -> bool {
        self.assoc.has_entry(lane, sid)
    }

    #[inline]
    pub(crate) fn active_lane_attachment_count(&self) -> usize {
        self.assoc.active_entry_count()
    }

    pub(crate) fn open_port_guard<'a>(
        &'a self,
        sid: SessionId,
        lane: Lane,
        role: u8,
        active_leases: &'a Cell<u32>,
    ) -> Result<(Port<'a, T>, LaneGuard<'a, T>), RendezvousError>
    where
        'rv: 'a,
    {
        let (tx, rx) = self
            .transport
            .open(crate::transport::PortOpen::from_descriptor(role, sid, lane));
        if self.session_fault(sid).is_some() {
            return Err(RendezvousError::SessionPoisoned { sid });
        }
        let port = Port::new(PortInit {
            transport: &self.transport,
            tap: self.tap(),
            tap_counter: &self.tap_counter,
            slab_ptr: self.slab_ptr,
            slab_len: self.slab_len,
            access_state: &self.access_state,
            image_frontier: &self.image_frontier,
            frontier_workspace_bytes: &self.frontier_workspace_bytes,
            endpoint_lease_storage: &self.endpoint_lease_storage,
            sid,
            lane,
            rv_id: self.id,
            tx,
            rx,
        });
        let guard =
            LaneGuard::new_detached((self as *const Self).cast::<()>(), sid, lane, active_leases);
        Ok((port, guard))
    }

    // ============================================================================
    // Port witness methods
    // ============================================================================
}
