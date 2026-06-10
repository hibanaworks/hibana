use super::{LocalTopologyInvariant, TopologyLeaseState, TopologyStateTable};
use crate::{
    control::automaton::txn::InAcked,
    control::types::{Generation, Lane, One, SessionId},
    rendezvous::error::TopologyError,
};

mod destination;
pub(crate) use destination::PreparedDestinationTopologyCommit;

pub(crate) struct PreparedSourceTopologyCommit {
    slot: u8,
    previous_generation: Option<Generation>,
    target: Generation,
    state: InAcked<LocalTopologyInvariant, One>,
}

impl PreparedSourceTopologyCommit {
    fn new(
        slot: usize,
        previous_generation: Option<Generation>,
        target: Generation,
        state: InAcked<LocalTopologyInvariant, One>,
    ) -> Self {
        assert!(slot <= u8::MAX as usize, "topology lane slot overflow");
        Self {
            slot: slot as u8,
            previous_generation,
            target,
            state,
        }
    }

    pub(in crate::rendezvous) const fn previous_generation(&self) -> Option<Generation> {
        self.previous_generation
    }

    pub(crate) const fn target(&self) -> Generation {
        self.target
    }

    pub(in crate::rendezvous) fn commit(self) {
        self.state.commit();
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
                        pending.previous_generation,
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
}
