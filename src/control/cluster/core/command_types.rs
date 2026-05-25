use super::*;

/// Control-plane effect envelope encompassing the effect and its operands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TopologyOperands {
    pub(crate) src_rv: RendezvousId,
    pub(crate) dst_rv: RendezvousId,
    pub(crate) src_lane: Lane,
    pub(crate) dst_lane: Lane,
    pub(crate) old_gen: Generation,
    pub(crate) new_gen: Generation,
    pub(crate) seq_tx: u32,
    pub(crate) seq_rx: u32,
}

impl TopologyOperands {
    pub(crate) fn intent(&self, sid: SessionId) -> TopologyIntent {
        TopologyIntent {
            src_rv: self.src_rv,
            dst_rv: self.dst_rv,
            sid: sid.raw(),
            old_gen: self.old_gen,
            new_gen: self.new_gen,
            seq_tx: self.seq_tx,
            seq_rx: self.seq_rx,
            src_lane: self.src_lane,
            dst_lane: self.dst_lane,
        }
    }

    pub(crate) fn ack(&self, sid: SessionId) -> TopologyAck {
        TopologyAck {
            src_rv: self.src_rv,
            dst_rv: self.dst_rv,
            sid: sid.raw(),
            new_gen: self.new_gen,
            src_lane: self.src_lane,
            new_lane: self.dst_lane,
            seq_tx: self.seq_tx,
            seq_rx: self.seq_rx,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DelegateOperands {
    pub claim: bool,
    pub token: GenericCapToken<EndpointResource>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PendingEffect {
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CpCommand {
    pub(crate) effect: ControlOp,
    pub(crate) sid: Option<SessionId>,
    pub(crate) lane: Option<Lane>,
    pub(crate) generation: Option<Generation>,
    pub(crate) topology: Option<TopologyOperands>,
    pub(crate) delegate: Option<DelegateOperands>,
}

impl CpCommand {
    pub(crate) const fn new(effect: ControlOp) -> Self {
        Self {
            effect,
            sid: None,
            lane: None,
            generation: None,
            topology: None,
            delegate: None,
        }
    }

    pub(crate) fn with_sid(mut self, sid: SessionId) -> Self {
        self.sid = Some(sid);
        self
    }

    pub(crate) fn with_lane(mut self, lane: Lane) -> Self {
        self.lane = Some(lane);
        self
    }

    pub(crate) fn with_generation(mut self, generation: Generation) -> Self {
        self.generation = Some(generation);
        self
    }

    pub(crate) fn with_topology(mut self, operands: TopologyOperands) -> Self {
        self.topology = Some(operands);
        self
    }

    pub(crate) fn with_delegate(mut self, delegate: DelegateOperands) -> Self {
        self.delegate = Some(delegate);
        self
    }

    fn derive_sid_lane(
        token: GenericCapToken<EndpointResource>,
    ) -> Result<(SessionId, Lane), CpError> {
        let handle = token
            .endpoint_identity()
            .map_err(|_| CpError::Delegation(DelegationError::InvalidToken))?;
        Ok((handle.sid, handle.lane))
    }

    pub(crate) fn canonicalize_delegate(mut self) -> Result<Self, CpError> {
        let delegate = self
            .delegate
            .ok_or(CpError::Delegation(DelegationError::InvalidToken))?;
        let (sid, lane) = Self::derive_sid_lane(delegate.token)?;
        if self.sid.is_some_and(|current| current != sid) {
            return Err(CpError::Delegation(DelegationError::InvalidToken));
        }
        if self.lane.is_some_and(|current| current != lane) {
            return Err(CpError::Delegation(DelegationError::InvalidToken));
        }
        self.sid = Some(sid);
        self.lane = Some(lane);
        self = self.with_delegate(DelegateOperands {
            claim: delegate.claim,
            token: delegate.token,
        });
        Ok(self)
    }

    pub(crate) fn canonicalize_topology(mut self) -> Result<Self, CpError> {
        let Some(operands) = self.topology else {
            return Err(CpError::Topology(TopologyError::InvalidState));
        };
        let (lane, generation) = match self.effect {
            ControlOp::TopologyBegin | ControlOp::TopologyCommit => {
                (operands.src_lane, operands.new_gen)
            }
            ControlOp::TopologyAck => (operands.dst_lane, operands.new_gen),
            _ => return Ok(self),
        };
        if let Some(current) = self.lane
            && current != lane
        {
            let _ = current;
            return Err(CpError::Topology(TopologyError::LaneMismatch));
        }
        if self.generation.is_some_and(|current| current != generation) {
            return Err(CpError::Topology(TopologyError::GenerationMismatch));
        }
        self.lane = Some(lane);
        self.generation = Some(generation);
        Ok(self)
    }

    pub(crate) fn topology_begin(sid: SessionId, operands: TopologyOperands) -> Self {
        Self::new(ControlOp::TopologyBegin)
            .with_sid(sid)
            .with_lane(operands.src_lane)
            .with_topology(operands)
    }

    pub(crate) fn topology_ack(sid: SessionId, operands: TopologyOperands) -> Self {
        Self::new(ControlOp::TopologyAck)
            .with_sid(sid)
            .with_lane(operands.dst_lane)
            .with_topology(operands)
    }

    pub(crate) fn topology_commit(sid: SessionId, operands: TopologyOperands) -> Self {
        Self::new(ControlOp::TopologyCommit)
            .with_sid(sid)
            .with_lane(operands.src_lane)
            .with_topology(operands)
    }

    pub(crate) fn abort_begin(sid: SessionId, lane: Lane) -> Self {
        Self::new(ControlOp::AbortBegin)
            .with_sid(sid)
            .with_lane(lane)
    }

    pub(crate) fn abort_ack(sid: SessionId, lane: Lane, generation: Generation) -> Self {
        Self::new(ControlOp::AbortAck)
            .with_sid(sid)
            .with_lane(lane)
            .with_generation(generation)
    }

    pub(crate) fn state_snapshot(sid: SessionId, lane: Lane) -> Self {
        Self::new(ControlOp::StateSnapshot)
            .with_sid(sid)
            .with_lane(lane)
    }

    pub(crate) fn state_restore(sid: SessionId, lane: Lane, generation: Generation) -> Self {
        Self::new(ControlOp::StateRestore)
            .with_sid(sid)
            .with_lane(lane)
            .with_generation(generation)
    }

    pub(crate) fn tx_commit(sid: SessionId, lane: Lane, generation: Generation) -> Self {
        Self::new(ControlOp::TxCommit)
            .with_sid(sid)
            .with_lane(lane)
            .with_generation(generation)
    }

    pub(crate) fn tx_abort(sid: SessionId, lane: Lane, generation: Generation) -> Self {
        Self::new(ControlOp::TxAbort)
            .with_sid(sid)
            .with_lane(lane)
            .with_generation(generation)
    }
}
