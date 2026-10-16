use super::{
    Cell, Lane, LaneGuard, Port, PortInit, Rendezvous, RendezvousError, SessionId, TapRing,
    Transport, emit, events,
};
impl<'rv, 'cfg, T: Transport> Rendezvous<'rv, 'cfg, T>
where
    'cfg: 'rv,
{
    /// Observability ring; pushing events only needs `&self` because the ring
    /// is single-producer and internally synchronised.
    pub(crate) fn tap(&self) -> &TapRing<'cfg> {
        &self.tap
    }

    pub(crate) fn transport(&self) -> &T {
        &self.transport
    }

    pub(crate) fn seal_session_membership(&self, sid: SessionId) {
        super::EndpointLeaseRecord::seal_session_membership(&self.endpoint_lease_storage, sid);
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
                events::lane_acquire(self.id.raw() as u32, sid.raw(), lane.as_wire() as u16),
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
            // SAFETY: the counted lane guard retains this pinned rendezvous.
            // Both invariant storage lifetimes are narrowed to the handle
            // borrow; no reference is extended past the caller's lane lease.
            rendezvous: unsafe { &*core::ptr::from_ref(self).cast::<Rendezvous<'a, 'a, T>>() },
            lane,
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
