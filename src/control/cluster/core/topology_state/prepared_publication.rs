use super::{DistributedEntry, DistributedPhase, DistributedTopologyState};
use crate::control::cluster::core::{
    CpError, DistributedTopology, DistributedTopologyInv, InAcked, InBegin, NoopTap, RendezvousId,
    SessionId, TopologyAck, TopologyError, TopologyOperands,
};

pub(crate) struct PreparedDistributedTopologyBegin {
    key: PreparedDistributedTopologyKey,
    txn: InBegin<DistributedTopologyInv, crate::control::types::One>,
}

pub(crate) struct PreparedDistributedTopologyAck {
    key: PreparedDistributedTopologyKey,
    txn: InBegin<DistributedTopologyInv, crate::control::types::One>,
}

pub(crate) struct PreparedDistributedTopologyCommit {
    key: PreparedDistributedTopologyKey,
    txn: InAcked<DistributedTopologyInv, crate::control::types::One>,
}

#[derive(Clone, Copy)]
struct PreparedDistributedTopologyKey {
    sid: SessionId,
    src_rv: RendezvousId,
}

impl PreparedDistributedTopologyKey {
    #[inline]
    const fn new(sid: SessionId, src_rv: RendezvousId) -> Self {
        Self { sid, src_rv }
    }
}

impl PreparedDistributedTopologyBegin {
    #[inline]
    const fn new(
        key: PreparedDistributedTopologyKey,
        txn: InBegin<DistributedTopologyInv, crate::control::types::One>,
    ) -> Self {
        Self { key, txn }
    }
}

impl PreparedDistributedTopologyAck {
    #[inline]
    const fn new(
        key: PreparedDistributedTopologyKey,
        txn: InBegin<DistributedTopologyInv, crate::control::types::One>,
    ) -> Self {
        Self { key, txn }
    }

    #[inline]
    pub(crate) const fn sid(&self) -> SessionId {
        self.key.sid
    }

    #[inline]
    fn acknowledge(self) -> InAcked<DistributedTopologyInv, crate::control::types::One> {
        let mut tap = NoopTap;
        DistributedTopology::acknowledge(self.txn, &mut tap)
    }
}

impl PreparedDistributedTopologyCommit {
    #[inline]
    const fn new(
        key: PreparedDistributedTopologyKey,
        txn: InAcked<DistributedTopologyInv, crate::control::types::One>,
    ) -> Self {
        Self { key, txn }
    }

    #[inline]
    pub(crate) const fn sid(&self) -> SessionId {
        self.key.sid
    }

    #[inline]
    fn commit(self) {
        let mut tap = NoopTap;
        DistributedTopology::topology_commit(self.txn, &mut tap);
    }
}

impl<const MAX: usize> DistributedTopologyState<MAX> {
    pub(crate) fn reserve_preflighted_begin(
        &mut self,
        sid: SessionId,
        operands: TopologyOperands,
    ) -> (TopologyAck, PreparedDistributedTopologyBegin) {
        debug_assert!(self.preflight_begin(sid, operands).is_ok());
        let mut tap = NoopTap;
        let (in_begin, _) = DistributedTopology::begin(operands.intent(sid), &mut tap);
        let entry = DistributedEntry {
            operands,
            phase: DistributedPhase::BeginReserved,
        };
        self.bucket_mut(operands.src_rv)
            .expect("topology begin preflight guaranteed source bucket")
            .insert(sid, entry)
            .expect("topology begin capacity reservation guaranteed a free slot");
        (
            operands.ack(sid),
            PreparedDistributedTopologyBegin::new(
                PreparedDistributedTopologyKey::new(sid, operands.src_rv),
                in_begin,
            ),
        )
    }

    pub(crate) fn rollback_prepared_begin(&mut self, ticket: PreparedDistributedTopologyBegin) {
        let sid = ticket.key.sid;
        let src_rv = ticket.key.src_rv;
        let removed = self
            .bucket_mut(src_rv)
            .and_then(|bucket| bucket.remove(sid))
            .expect("distributed topology begin rollback missing prepared reservation");
        assert!(
            matches!(removed.phase, DistributedPhase::BeginReserved),
            "distributed topology begin rollback found non-reserved phase"
        );
    }

    pub(crate) fn publish_prepared_begin(&mut self, ticket: PreparedDistributedTopologyBegin) {
        let sid = ticket.key.sid;
        let src_rv = ticket.key.src_rv;
        let removed = self
            .bucket_mut(src_rv)
            .and_then(|bucket| bucket.remove(sid))
            .expect("distributed topology begin publish missing prepared reservation");
        let entry = removed;
        let DistributedEntry { operands, phase } = entry;
        assert!(
            matches!(phase, DistributedPhase::BeginReserved),
            "distributed topology begin publish found non-reserved phase"
        );
        self.bucket_mut(src_rv)
            .expect("distributed topology begin publish owner bucket missing")
            .insert(
                sid,
                DistributedEntry {
                    operands,
                    phase: DistributedPhase::Begin { txn: ticket.txn },
                },
            )
            .expect("distributed topology begin publish could not restore prepared entry");
    }

    pub(crate) fn reserve_preflighted_ack(
        &mut self,
        sid: SessionId,
        src_rv: RendezvousId,
        expected: TopologyAck,
    ) -> PreparedDistributedTopologyAck {
        let entry = self
            .bucket_mut(src_rv)
            .and_then(|bucket| bucket.remove(sid))
            .expect("distributed topology ack reservation missing preflighted begin entry");
        let DistributedEntry { operands, phase } = entry;
        assert_eq!(
            operands.ack(sid),
            expected,
            "distributed topology ack reservation diverged from preflighted operands"
        );
        let DistributedPhase::Begin { txn } = phase else {
            panic!("distributed topology ack reservation found non-begin phase");
        };
        self.bucket_mut(src_rv)
            .expect("distributed topology ack reservation owner bucket missing")
            .insert(
                sid,
                DistributedEntry {
                    operands,
                    phase: DistributedPhase::AckReserved,
                },
            )
            .expect("distributed topology ack reservation could not restore prepared entry");
        PreparedDistributedTopologyAck::new(PreparedDistributedTopologyKey::new(sid, src_rv), txn)
    }

    pub(crate) fn publish_prepared_ack(&mut self, ticket: PreparedDistributedTopologyAck) {
        let sid = ticket.key.sid;
        let src_rv = ticket.key.src_rv;
        let removed = self
            .bucket_mut(src_rv)
            .and_then(|bucket| bucket.remove(sid))
            .expect("distributed topology ack publish missing prepared reservation");
        let entry = removed;
        let DistributedEntry { operands, phase } = entry;
        assert!(
            matches!(phase, DistributedPhase::AckReserved),
            "distributed topology ack publish found non-reserved phase"
        );
        self.bucket_mut(src_rv)
            .expect("distributed topology ack publish owner bucket missing")
            .insert(
                sid,
                DistributedEntry {
                    operands,
                    phase: DistributedPhase::Acked {
                        txn: ticket.acknowledge(),
                    },
                },
            )
            .expect("distributed topology ack publish could not restore prepared entry");
    }

    pub(crate) fn rollback_prepared_ack(&mut self, ticket: PreparedDistributedTopologyAck) {
        let sid = ticket.key.sid;
        let src_rv = ticket.key.src_rv;
        let removed = self
            .bucket_mut(src_rv)
            .and_then(|bucket| bucket.remove(sid))
            .expect("distributed topology ack rollback missing prepared reservation");
        let entry = removed;
        let DistributedEntry { operands, phase } = entry;
        assert!(
            matches!(phase, DistributedPhase::AckReserved),
            "distributed topology ack rollback found non-reserved phase"
        );
        self.bucket_mut(src_rv)
            .expect("distributed topology ack rollback owner bucket missing")
            .insert(
                sid,
                DistributedEntry {
                    operands,
                    phase: DistributedPhase::Begin { txn: ticket.txn },
                },
            )
            .expect("distributed topology ack rollback could not restore entry");
    }

    pub(crate) fn reserve_commit(
        &mut self,
        sid: SessionId,
        src_rv: RendezvousId,
        expected: Option<TopologyAck>,
    ) -> Result<PreparedDistributedTopologyCommit, CpError> {
        self.preflight_commit(sid, src_rv, expected)?;
        let entry = self
            .bucket_mut(src_rv)
            .and_then(|bucket| bucket.remove(sid))
            .ok_or(CpError::Topology(TopologyError::InvalidSession))?;
        let DistributedEntry { operands, phase } = entry;
        match phase {
            DistributedPhase::Acked { txn } => {
                self.bucket_mut(src_rv)
                    .ok_or(CpError::RendezvousMismatch {
                        expected: src_rv.raw(),
                        actual: 0,
                    })?
                    .insert(
                        sid,
                        DistributedEntry {
                            operands,
                            phase: DistributedPhase::CommitReserved,
                        },
                    )?;
                Ok(PreparedDistributedTopologyCommit::new(
                    PreparedDistributedTopologyKey::new(sid, src_rv),
                    txn,
                ))
            }
            other => {
                self.bucket_mut(src_rv)
                    .ok_or(CpError::RendezvousMismatch {
                        expected: src_rv.raw(),
                        actual: 0,
                    })?
                    .insert(
                        sid,
                        DistributedEntry {
                            operands,
                            phase: other,
                        },
                    )?;
                Err(CpError::Topology(TopologyError::InvalidState))
            }
        }
    }

    pub(crate) fn rollback_commit_reserved(&mut self, ticket: PreparedDistributedTopologyCommit) {
        let sid = ticket.key.sid;
        let src_rv = ticket.key.src_rv;
        let entry = self
            .bucket_mut(src_rv)
            .and_then(|bucket| bucket.remove(sid))
            .expect("distributed topology commit rollback missing reservation");
        let DistributedEntry { operands, phase } = entry;
        assert!(
            matches!(phase, DistributedPhase::CommitReserved),
            "distributed topology commit rollback found non-reserved phase"
        );
        self.bucket_mut(src_rv)
            .expect("distributed topology commit rollback owner bucket missing")
            .insert(
                sid,
                DistributedEntry {
                    operands,
                    phase: DistributedPhase::Acked { txn: ticket.txn },
                },
            )
            .expect("distributed topology commit rollback could not restore entry");
    }

    pub(crate) fn assert_prepared_commit(&self, ticket: &PreparedDistributedTopologyCommit) {
        let entry = self
            .bucket(ticket.key.src_rv)
            .and_then(|bucket| bucket.get(ticket.key.sid))
            .unwrap();
        assert!(matches!(entry.phase, DistributedPhase::CommitReserved));
    }

    pub(crate) fn publish_prepared_commit(&mut self, ticket: PreparedDistributedTopologyCommit) {
        let sid = ticket.key.sid;
        let src_rv = ticket.key.src_rv;
        let _ = self.bucket_mut(src_rv).unwrap().remove(sid).unwrap();
        ticket.commit();
    }
}
