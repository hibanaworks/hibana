use super::{Generation, Lane, RendezvousId, SessionId, TopologyAck, TopologyIntent};

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
