use super::commit_delta::PreparedCommitDelta;
use super::{
    CursorEndpoint, EndpointArenaLayout, LaneGuard, Payload, Port, SendError, SendMeta,
    SendPreview, SendResult, StateIndex, Transport, lane_port,
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
    pub(in crate::endpoint::kernel) transport: lane_port::PendingSend<'r>,
    pub(in crate::endpoint::kernel) commit_plan: Option<SendCommitPlan<'r>>,
}

impl<'r> PendingSendIo<'r> {
    #[inline(always)]
    pub(in crate::endpoint::kernel) fn lane_idx(&self) -> usize {
        self.transport.lane_idx()
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

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RecvPayloadMode {
    RequiresPayload = 0,
    AllowsZeroLength = 1,
}

impl RecvPayloadMode {
    #[inline]
    pub(crate) const fn from_allows_zero_length(allows_zero_length: bool) -> Self {
        if allows_zero_length {
            Self::AllowsZeroLength
        } else {
            Self::RequiresPayload
        }
    }

    #[inline]
    pub(crate) const fn allows_zero_length(self) -> bool {
        matches!(self, Self::AllowsZeroLength)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct MsgFlags(u8);

impl MsgFlags {
    const ALLOWS_ZERO_LENGTH: u8 = 1 << 0;

    #[inline(always)]
    pub(crate) const fn new(payload_mode: RecvPayloadMode) -> Self {
        let mut bits = 0u8;
        if payload_mode.allows_zero_length() {
            bits |= Self::ALLOWS_ZERO_LENGTH;
        }
        Self(bits)
    }

    #[inline(always)]
    pub(crate) const fn payload_mode(self) -> RecvPayloadMode {
        if self.0 & Self::ALLOWS_ZERO_LENGTH != 0 {
            RecvPayloadMode::AllowsZeroLength
        } else {
            RecvPayloadMode::RequiresPayload
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct MsgCore {
    logical_label: crate::transport::LogicalLabel,
    frame_label: crate::transport::FrameLabel,
    flags: MsgFlags,
}

impl MsgCore {
    #[inline]
    pub(crate) const fn new(
        logical_label: u8,
        frame_label: crate::transport::FrameLabel,
        payload_mode: RecvPayloadMode,
    ) -> Self {
        Self {
            logical_label: crate::transport::LogicalLabel::new(logical_label),
            frame_label,
            flags: MsgFlags::new(payload_mode),
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

    #[inline]
    pub(crate) const fn payload_mode(self) -> RecvPayloadMode {
        self.flags.payload_mode()
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct RecvRuntimeDesc {
    pub(crate) core: MsgCore,
}

impl RecvRuntimeDesc {
    #[inline]
    pub(crate) const fn new(
        logical_label: u8,
        frame_label: crate::transport::FrameLabel,
        payload_mode: RecvPayloadMode,
    ) -> Self {
        Self {
            core: MsgCore::new(logical_label, frame_label, payload_mode),
        }
    }

    #[inline]
    pub(crate) const fn frame_label(self) -> crate::transport::FrameLabel {
        self.core.frame_label()
    }

    #[inline]
    pub(crate) const fn payload_mode(self) -> RecvPayloadMode {
        self.core.payload_mode()
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct DecodeRuntimeDesc {
    pub(crate) core: MsgCore,
    validate: for<'a> fn(Payload<'a>) -> Result<(), crate::transport::wire::CodecError>,
    zero_payload:
        for<'a> fn(&'a mut [u8]) -> Result<Payload<'a>, crate::transport::wire::CodecError>,
}

impl DecodeRuntimeDesc {
    #[inline]
    pub(crate) const fn new(
        logical_label: u8,
        frame_label: crate::transport::FrameLabel,
        validate: for<'a> fn(Payload<'a>) -> Result<(), crate::transport::wire::CodecError>,
        zero_payload: for<'a> fn(
            &'a mut [u8],
        )
            -> Result<Payload<'a>, crate::transport::wire::CodecError>,
    ) -> Self {
        Self {
            core: MsgCore::new(logical_label, frame_label, RecvPayloadMode::RequiresPayload),
            validate,
            zero_payload,
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

    #[inline]
    pub(crate) fn zero_payload<'a>(
        self,
        scratch: &'a mut [u8],
    ) -> Result<Payload<'a>, crate::transport::wire::CodecError> {
        (self.zero_payload)(scratch)
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct SendRuntimeDesc {
    pub(crate) core: MsgCore,
    encode_payload:
        unsafe fn(*const (), &mut [u8]) -> Result<usize, crate::transport::wire::CodecError>,
}

impl SendRuntimeDesc {
    #[inline]
    pub(crate) const fn new(
        logical_label: u8,
        frame_label: crate::transport::FrameLabel,
        encode_payload: unsafe fn(
            *const (),
            &mut [u8],
        ) -> Result<usize, crate::transport::wire::CodecError>,
    ) -> Self {
        Self {
            core: MsgCore::new(logical_label, frame_label, RecvPayloadMode::RequiresPayload),
            encode_payload,
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
    pub(crate) fn encode_payload(
        self,
        payload: lane_port::RawSendPayload,
        scratch: &mut [u8],
    ) -> Result<usize, SendError> {
        payload.encode_into(self.encode_payload, scratch)
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
pub(crate) const fn cursor_endpoint_storage_layout<'r, const ROLE: u8, T, C, const MAX_RV: usize>(
    arena_layout: &EndpointArenaLayout,
    lane_slot_count: usize,
) -> CursorEndpointStorageLayout
where
    T: Transport + 'r,
    C: crate::runtime_core::config::Clock + 'r,
{
    let header_bytes = core::mem::size_of::<CursorEndpoint<'r, ROLE, T, C, MAX_RV>>();
    let header_align = core::mem::align_of::<CursorEndpoint<'r, ROLE, T, C, MAX_RV>>();
    let port_slots_align = core::mem::align_of::<Option<Port<'r, T>>>();
    let port_slots_bytes =
        storage_checked_mul(core::mem::size_of::<Option<Port<'r, T>>>(), lane_slot_count);
    let port_slots_offset = storage_align_up(header_bytes, port_slots_align);
    let guard_slots_align = core::mem::align_of::<Option<LaneGuard<'r, T, C>>>();
    let guard_slots_bytes = storage_checked_mul(
        core::mem::size_of::<Option<LaneGuard<'r, T, C>>>(),
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
