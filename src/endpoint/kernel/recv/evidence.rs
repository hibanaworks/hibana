use super::lane_port;
use crate::global::typestate::{RecvMeta, StateIndex};

pub(crate) struct MatchedRecvFrame<'a> {
    pub(super) desc: RecvDescriptor,
    pub(super) frame: lane_port::ReceivedFrame<'a>,
}

#[derive(Clone, Copy)]
pub(super) struct RecvCandidate {
    pub(super) desc: RecvDescriptor,
}

#[derive(Clone, Copy)]
pub(super) struct RecvDescriptor {
    pub(super) meta: RecvMeta,
    pub(super) cursor_index: StateIndex,
    pub(super) sid_raw: u32,
    pub(super) lane_idx: usize,
    pub(super) lane_wire: u8,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct ObservedInboundKey {
    session_raw: u32,
    lane_wire: u8,
    source_role: u8,
    target_role: u8,
    frame_label: u8,
}

#[derive(Clone, Copy)]
pub(super) enum MatchAccumulator {
    None,
    One(RecvCandidate),
    Ambiguous,
}

#[derive(Clone, Copy)]
pub(super) enum MatchOutcome {
    None,
    Ambiguous,
}

impl ObservedInboundKey {
    #[inline]
    pub(super) fn from_frame<const ROLE: u8>(
        sid_raw: u32,
        frame: &lane_port::PreambleFrame<'_>,
    ) -> Self {
        let observation = frame.observed_transport_frame(sid_raw, frame.lane_wire(), ROLE);
        Self {
            session_raw: observation.session_raw(),
            lane_wire: observation.lane_wire(),
            source_role: observation.source_role(),
            target_role: observation.target_role(),
            frame_label: observation.label_raw(),
        }
    }

    #[inline]
    pub(super) const fn matches_recv_meta(
        self,
        sid_raw: u32,
        lane_wire: u8,
        local_role: u8,
        meta: RecvMeta,
    ) -> bool {
        self.session_raw == sid_raw
            && self.lane_wire == lane_wire
            && self.source_role == meta.peer
            && self.target_role == local_role
            && self.frame_label == meta.frame_label
    }
}

impl MatchAccumulator {
    #[inline]
    pub(super) const fn add(self, candidate: RecvCandidate) -> Self {
        match self {
            MatchAccumulator::None => MatchAccumulator::One(candidate),
            MatchAccumulator::One(_) | MatchAccumulator::Ambiguous => MatchAccumulator::Ambiguous,
        }
    }

    #[inline]
    pub(super) const fn finish(self) -> Result<RecvCandidate, MatchOutcome> {
        match self {
            MatchAccumulator::None => Err(MatchOutcome::None),
            MatchAccumulator::One(candidate) => Ok(candidate),
            MatchAccumulator::Ambiguous => Err(MatchOutcome::Ambiguous),
        }
    }
}
