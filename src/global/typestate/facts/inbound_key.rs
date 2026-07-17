//! Pure inbound operation identities shared by receive admission paths.

use super::RecvMeta;

/// Complete descriptor-visible identity of a framed inbound operation.
///
/// Session and target role are fixed by the attached endpoint. These three
/// fields are the remaining wire facts needed to select exactly one receive
/// occurrence without conflating messages from different peers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct InboundFrameKey {
    pub(crate) source_role: u8,
    pub(crate) lane: u8,
    pub(crate) frame_label: u8,
}

impl InboundFrameKey {
    #[inline(always)]
    pub(crate) const fn new(source_role: u8, lane: u8, frame_label: u8) -> Self {
        Self {
            source_role,
            lane,
            frame_label,
        }
    }

    #[inline(always)]
    pub(crate) const fn matches_recv(self, meta: RecvMeta) -> bool {
        self.matches_parts(meta.peer, meta.lane, meta.frame_label)
    }

    #[inline(always)]
    pub(super) const fn matches_parts(self, source_role: u8, lane: u8, frame_label: u8) -> bool {
        self.source_role == source_role && self.lane == lane && self.frame_label == frame_label
    }
}

/// Complete descriptor-visible identity available to headerless ingress.
///
/// The lane-bound transport handle supplies `lane`; the endpoint operation
/// supplies logical label and schema. Peer and frame label are intentionally
/// absent because a deterministic frame does not observe them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DeterministicInboundKey {
    pub(crate) lane: u8,
    pub(crate) label: u8,
    pub(crate) schema: u32,
}

impl DeterministicInboundKey {
    #[inline(always)]
    pub(crate) const fn new(lane: u8, label: u8, schema: u32) -> Self {
        Self {
            lane,
            label,
            schema,
        }
    }

    #[inline(always)]
    pub(crate) const fn matches_recv(self, meta: RecvMeta) -> bool {
        self.matches_parts(meta.lane, meta.label, meta.payload_schema)
    }

    #[inline(always)]
    pub(super) const fn matches_parts(self, lane: u8, label: u8, schema: u32) -> bool {
        self.lane == lane && self.label == label && self.schema == schema
    }
}
