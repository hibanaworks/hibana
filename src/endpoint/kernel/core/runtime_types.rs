use super::commit_delta::PreparedCommitDelta;
use super::{
    CursorEndpoint, EndpointArenaLayout, Lane, LaneGuard, Payload, Port, SendMeta, SendPreview,
    SendResult, StateIndex, Transport, lane_port,
};

pub(crate) struct StagedSendPayload {
    pub(crate) encoded_len: usize,
}

pub(in crate::endpoint::kernel::core) struct SendProgressCommitPlan {
    pub(crate) delta: PreparedCommitDelta,
}

mod commit;
pub(crate) use commit::{CommitDelta, CommitEventRow, CommitRow};

pub(crate) struct SendCommitProof<'rv> {
    pub(in crate::endpoint::kernel::core) progress: SendProgressCommitPlan,
    pub(in crate::endpoint::kernel::core) _borrow: core::marker::PhantomData<&'rv ()>,
}

pub(crate) struct SendCommitPlan<'rv> {
    pub(in crate::endpoint::kernel::core) proof: SendCommitProof<'rv>,
}

pub(crate) struct PendingSendIo<'r> {
    pub(in crate::endpoint::kernel) lane: Lane,
    pub(in crate::endpoint::kernel) transport: lane_port::PendingSend<'r>,
    pub(in crate::endpoint::kernel) commit_plan: Option<SendCommitPlan<'r>>,
}

impl<'r> PendingSendIo<'r> {
    #[inline(always)]
    pub(in crate::endpoint::kernel) fn lane_idx(&self) -> usize {
        self.lane.raw() as usize
    }

    #[inline(always)]
    pub(in crate::endpoint::kernel) fn lane_wire(&self) -> u8 {
        self.lane.as_wire()
    }
}

pub(crate) enum SendTransportStep<'r> {
    Immediate(SendCommitPlan<'r>),
    Pending(PendingSendIo<'r>),
}

pub(crate) enum SendInitOutcome<'r> {
    Ready(SendResult<SendCommitOutcome<'r>>),
    Pending { pending: PendingSendIo<'r> },
    Commit { commit_plan: SendCommitPlan<'r> },
}

pub(crate) struct SendCommitOutcome<'rv> {
    pub(crate) _borrow: core::marker::PhantomData<&'rv ()>,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct MsgCore {
    logical_label: crate::transport::LogicalLabel,
    frame_label: crate::transport::FrameLabel,
}

impl MsgCore {
    #[inline]
    pub(crate) const fn new(logical_label: u8, frame_label: crate::transport::FrameLabel) -> Self {
        Self {
            logical_label: crate::transport::LogicalLabel::new(logical_label),
            frame_label,
        }
    }

    #[inline]
    pub(crate) const fn logical_label(self) -> u8 {
        self.logical_label.raw()
    }

    #[inline]
    pub(crate) const fn frame_label(self) -> crate::transport::FrameLabel {
        self.frame_label
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct RecvRuntimeDesc {
    pub(crate) core: MsgCore,
}

impl RecvRuntimeDesc {
    #[inline]
    pub(crate) const fn new(logical_label: u8, frame_label: crate::transport::FrameLabel) -> Self {
        Self {
            core: MsgCore::new(logical_label, frame_label),
        }
    }

    #[inline]
    pub(crate) const fn frame_label(self) -> crate::transport::FrameLabel {
        self.core.frame_label()
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct BranchRecvRuntimeDesc {
    pub(crate) core: MsgCore,
    validate: for<'a> fn(Payload<'a>) -> Result<(), crate::transport::wire::CodecError>,
}

impl BranchRecvRuntimeDesc {
    #[inline]
    pub(crate) const fn new(
        logical_label: u8,
        frame_label: crate::transport::FrameLabel,
        validate: for<'a> fn(Payload<'a>) -> Result<(), crate::transport::wire::CodecError>,
    ) -> Self {
        Self {
            core: MsgCore::new(logical_label, frame_label),
            validate,
        }
    }

    #[inline]
    pub(crate) const fn logical_label(self) -> u8 {
        self.core.logical_label()
    }

    #[inline]
    pub(crate) const fn frame_label(self) -> crate::transport::FrameLabel {
        self.core.frame_label()
    }

    #[inline]
    pub(crate) fn validate_payload(
        self,
        payload: Payload<'_>,
    ) -> Result<(), crate::transport::wire::CodecError> {
        (self.validate)(payload)
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct SendRuntimeDesc {
    pub(crate) core: MsgCore,
}

impl SendRuntimeDesc {
    #[inline]
    pub(crate) const fn new(logical_label: u8, frame_label: crate::transport::FrameLabel) -> Self {
        Self {
            core: MsgCore::new(logical_label, frame_label),
        }
    }

    #[inline]
    pub(crate) const fn logical_label(self) -> u8 {
        self.core.logical_label()
    }

    #[inline]
    pub(crate) const fn frame_label(self) -> crate::transport::FrameLabel {
        self.core.frame_label()
    }
}

#[derive(Clone, Copy)]
pub(crate) struct SendInit {
    pub(in crate::endpoint::kernel) descriptor: SendRuntimeDesc,
    pub(in crate::endpoint::kernel) preview: SendPreview,
}

impl SendInit {
    #[inline]
    pub(crate) const fn new(descriptor: SendRuntimeDesc, preview: SendPreview) -> Self {
        Self {
            descriptor,
            preview,
        }
    }
}

pub(crate) enum SendState<'r> {
    Init {
        descriptor: SendRuntimeDesc,
        meta: SendMeta,
        preview_cursor_index: Option<StateIndex>,
    },
    Sending {
        pending: PendingSendIo<'r>,
    },
    Done,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CursorEndpointStorageLayout {
    header_bytes: usize,
    header_align: usize,
    port_slots_offset: usize,
    port_slots_bytes: usize,
    port_slots_align: usize,
    guard_slots_offset: usize,
    guard_slots_bytes: usize,
    guard_slots_align: usize,
    arena_offset: usize,
    arena_bytes: usize,
    arena_align: usize,
    total_bytes: usize,
    total_align: usize,
}

impl CursorEndpointStorageLayout {
    #[inline(always)]
    pub(crate) const fn port_slots_offset(self) -> usize {
        self.port_slots_offset
    }

    #[inline(always)]
    pub(crate) const fn guard_slots_offset(self) -> usize {
        self.guard_slots_offset
    }

    #[inline(always)]
    pub(crate) const fn arena_offset(self) -> usize {
        self.arena_offset
    }

    #[inline(always)]
    pub(crate) const fn total_bytes(self) -> usize {
        self.total_bytes
    }

    #[inline(always)]
    pub(crate) const fn total_align(self) -> usize {
        self.total_align
    }
}

#[inline(always)]
const fn storage_align_up(value: usize, align: usize) -> usize {
    if align == 0 {
        crate::invariant();
    }
    let mask = align - 1;
    if value > usize::MAX - mask {
        crate::invariant();
    }
    (value + mask) & !mask
}

#[inline(always)]
const fn storage_max(lhs: usize, rhs: usize) -> usize {
    if lhs > rhs { lhs } else { rhs }
}

#[inline(always)]
const fn storage_checked_add(lhs: usize, rhs: usize) -> usize {
    if lhs > usize::MAX - rhs {
        crate::invariant();
    }
    lhs + rhs
}

#[inline(always)]
const fn storage_checked_mul(lhs: usize, rhs: usize) -> usize {
    if lhs != 0 && rhs > usize::MAX / lhs {
        crate::invariant();
    }
    lhs * rhs
}

#[inline]
pub(crate) const fn cursor_endpoint_storage_layout<'r, const ROLE: u8, T>(
    arena_layout: &EndpointArenaLayout,
    lane_slot_count: usize,
) -> CursorEndpointStorageLayout
where
    T: Transport + 'r,
{
    let header_bytes = core::mem::size_of::<CursorEndpoint<'r, ROLE, T>>();
    let header_align = core::mem::align_of::<CursorEndpoint<'r, ROLE, T>>();
    let port_slots_align = core::mem::align_of::<Option<Port<'r, T>>>();
    let port_slots_bytes =
        storage_checked_mul(core::mem::size_of::<Option<Port<'r, T>>>(), lane_slot_count);
    let port_slots_offset = storage_align_up(header_bytes, port_slots_align);
    let guard_slots_align = core::mem::align_of::<Option<LaneGuard<'r, T>>>();
    let guard_slots_bytes = storage_checked_mul(
        core::mem::size_of::<Option<LaneGuard<'r, T>>>(),
        lane_slot_count,
    );
    let guard_slots_offset = storage_align_up(
        storage_checked_add(port_slots_offset, port_slots_bytes),
        guard_slots_align,
    );
    let arena_offset = storage_align_up(
        storage_checked_add(guard_slots_offset, guard_slots_bytes),
        arena_layout.header_align(),
    );
    let total_align = storage_max(
        storage_max(
            storage_max(header_align, port_slots_align),
            guard_slots_align,
        ),
        arena_layout.header_align(),
    );
    CursorEndpointStorageLayout {
        header_bytes,
        header_align,
        port_slots_offset,
        port_slots_bytes,
        port_slots_align,
        guard_slots_offset,
        guard_slots_bytes,
        guard_slots_align,
        arena_offset,
        arena_bytes: arena_layout.total_bytes(),
        arena_align: arena_layout.total_align(),
        total_bytes: storage_checked_add(arena_offset, arena_layout.total_bytes()),
        total_align,
    }
}

#[cfg(test)]
mod size_tests {
    use super::*;

    #[test]
    fn pending_send_commit_proof_stays_compact() {
        assert!(
            core::mem::size_of::<SendCommitProof<'static>>() <= 96,
            "SendCommitProof grew to {} bytes",
            core::mem::size_of::<SendCommitProof<'static>>()
        );
        assert!(
            core::mem::size_of::<SendCommitOutcome<'static>>() <= 56,
            "SendCommitOutcome grew to {} bytes",
            core::mem::size_of::<SendCommitOutcome<'static>>()
        );
        assert!(
            core::mem::size_of::<PendingSendIo<'static>>() <= 184,
            "PendingSendIo grew to {} bytes",
            core::mem::size_of::<PendingSendIo<'static>>()
        );
    }
}
