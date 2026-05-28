use super::{LocalTopologyInvariant, TopologyLeaseState, TopologyStateTable};
use crate::{
    control::automaton::txn::{InAcked, NoopTap},
    control::types::{Generation, Lane, One, SessionId},
    rendezvous::error::TopologyError,
};

pub(crate) struct PreparedSourceTopologyCommit {
    slot: u16,
    target: Generation,
    state: InAcked<LocalTopologyInvariant, One>,
}

pub(crate) struct PreparedDestinationTopologyCommit {
    slot: u16,
    previous_generation: Option<Generation>,
    target: Generation,
}

impl PreparedSourceTopologyCommit {
    fn new(slot: usize, target: Generation, state: InAcked<LocalTopologyInvariant, One>) -> Self {
        Self {
            slot: slot as u16,
            target,
            state,
        }
    }

    pub(in crate::rendezvous) const fn target(&self) -> Generation {
        self.target
    }

    pub(in crate::rendezvous) fn commit(self) {
        let mut tap = NoopTap;
        self.state.commit(&mut tap);
    }
}

impl PreparedDestinationTopologyCommit {
    const fn new(slot: usize, previous_generation: Option<Generation>, target: Generation) -> Self {
        Self {
            slot: slot as u16,
            previous_generation,
            target,
        }
    }

    pub(in crate::rendezvous) const fn previous_generation(&self) -> Option<Generation> {
        self.previous_generation
    }

    pub(in crate::rendezvous) const fn target(&self) -> Generation {
        self.target
    }
}

impl TopologyStateTable {
    pub(in crate::rendezvous) fn reserve_source_commit(
        &self,
        lane: Lane,
        sid: SessionId,
    ) -> Result<PreparedSourceTopologyCommit, TopologyError> {
        let slots = self.lanes_ptr();
        let Some(idx) = self.lane_slot(lane) else {
            return Err(TopologyError::NoPending { lane });
        };
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe {
            match (&mut *slots.add(idx)).as_mut() {
                Some(pending)
                    if pending.sid == sid
                        && pending.lease_state == TopologyLeaseState::SourcePrepared =>
                {
                    let Some(state) = pending.state.take() else {
                        return Err(TopologyError::NoPending { lane });
                    };
                    pending.lease_state = TopologyLeaseState::SourceCommitReserved;
                    Ok(PreparedSourceTopologyCommit::new(
                        idx,
                        pending.target,
                        state,
                    ))
                }
                Some(pending) if pending.sid == sid => Err(TopologyError::InProgress { lane }),
                Some(pending) => Err(TopologyError::UnknownSession { sid: pending.sid }),
                None => Err(TopologyError::NoPending { lane }),
            }
        }
    }

    pub(in crate::rendezvous) fn rollback_source_commit_reserved(
        &self,
        lane: Lane,
        sid: SessionId,
        ticket: PreparedSourceTopologyCommit,
    ) {
        let slots = self.lanes_ptr();
        let idx = self.lane_slot(lane).unwrap();
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe {
            let slot = &mut *slots.add(idx);
            let pending = slot.as_mut().unwrap();
            assert_eq!(pending.sid, sid);
            assert_eq!(
                pending.lease_state,
                TopologyLeaseState::SourceCommitReserved
            );
            assert!(pending.state.is_none());
            pending.lease_state = TopologyLeaseState::SourcePrepared;
            pending.state = Some(ticket.state);
        }
    }

    pub(in crate::rendezvous) fn reserve_destination_commit(
        &self,
        lane: Lane,
        sid: SessionId,
    ) -> Result<PreparedDestinationTopologyCommit, TopologyError> {
        let slots = self.lanes_ptr();
        let Some(idx) = self.lane_slot(lane) else {
            return Err(TopologyError::NoPending { lane });
        };
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe {
            match (&mut *slots.add(idx)).as_mut() {
                Some(pending)
                    if pending.sid == sid
                        && pending.lease_state == TopologyLeaseState::DestinationPrepared =>
                {
                    pending.lease_state = TopologyLeaseState::DestinationCommitReserved;
                    Ok(PreparedDestinationTopologyCommit::new(
                        idx,
                        pending.previous_generation,
                        pending.target,
                    ))
                }
                Some(pending) if pending.sid == sid => Err(TopologyError::InProgress { lane }),
                Some(pending) => Err(TopologyError::UnknownSession { sid: pending.sid }),
                None => Err(TopologyError::NoPending { lane }),
            }
        }
    }

    pub(in crate::rendezvous) fn rollback_destination_commit_reserved(
        &self,
        lane: Lane,
        sid: SessionId,
        _ticket: PreparedDestinationTopologyCommit,
    ) {
        let slots = self.lanes_ptr();
        let idx = self.lane_slot(lane).unwrap();
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe {
            let pending = (&mut *slots.add(idx)).as_mut().unwrap();
            assert_eq!(pending.sid, sid);
            assert_eq!(
                pending.lease_state,
                TopologyLeaseState::DestinationCommitReserved
            );
            pending.lease_state = TopologyLeaseState::DestinationPrepared;
        }
    }

    pub(in crate::rendezvous) fn assert_source_commit_reserved(
        &self,
        lane: Lane,
        sid: SessionId,
        ticket: &PreparedSourceTopologyCommit,
    ) {
        let slot = ticket.slot as usize;
        assert_eq!(self.lane_slot(lane), Some(slot));
        let slots = self.lanes_ptr();
        /* SAFETY: the prepared proof carries the slot minted by `reserve_source_commit`. */
        unsafe {
            let pending = (&*slots.add(slot)).as_ref().unwrap();
            assert_eq!(pending.sid, sid);
            assert_eq!(
                pending.lease_state,
                TopologyLeaseState::SourceCommitReserved
            );
            assert_eq!(pending.target, ticket.target);
            assert!(pending.state.is_none());
        }
    }

    pub(in crate::rendezvous) fn clear_prepared_source_commit_unchecked(
        &self,
        ticket: &PreparedSourceTopologyCommit,
    ) {
        let slots = self.lanes_ptr();
        /* SAFETY: `assert_source_commit_reserved` is run before the first
        irreversible publish mutation for the same prepared proof. */
        unsafe {
            *slots.add(ticket.slot as usize) = None;
        }
    }

    pub(in crate::rendezvous) fn assert_destination_commit_reserved(
        &self,
        lane: Lane,
        sid: SessionId,
        ticket: &PreparedDestinationTopologyCommit,
    ) {
        let slot = ticket.slot as usize;
        assert_eq!(self.lane_slot(lane), Some(slot));
        let slots = self.lanes_ptr();
        /* SAFETY: the prepared proof carries the slot minted by
        `reserve_destination_commit`. */
        unsafe {
            let pending = (&*slots.add(slot)).as_ref().unwrap();
            assert_eq!(pending.sid, sid);
            assert_eq!(
                pending.lease_state,
                TopologyLeaseState::DestinationCommitReserved
            );
            assert_eq!(pending.target, ticket.target);
        }
    }

    pub(in crate::rendezvous) fn finalize_prepared_destination_commit_unchecked(
        &self,
        ticket: PreparedDestinationTopologyCommit,
    ) {
        let slots = self.lanes_ptr();
        /* SAFETY: `assert_destination_commit_reserved` is run before the first
        irreversible publish mutation for the same prepared proof. */
        unsafe {
            let pending = (&mut *slots.add(ticket.slot as usize))
                .as_mut()
                .unwrap_unchecked();
            pending.lease_state = TopologyLeaseState::DestinationCommitted;
            pending.state = None;
        }
    }
}
