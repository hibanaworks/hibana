use super::{
    CAP_HANDLE_LEN, ControlDesc, CursorEndpoint, EndpointArenaLayout, EpochTable, LabelUniverse,
    Lane, LaneGuard, MintConfigMarker, Payload, PendingCapRelease, Port, SendDescriptorPublication,
    SendDescriptorTerminal, SendError, SendMeta, SendPreview, SendResult, SessionId, StateIndex,
    Transport, lane_port,
};

pub(crate) enum StagedControlEmission<'rv> {
    None,
    Registered(PendingCapRelease<'rv>),
    WireOnly,
}

pub(crate) struct StagedSendPayload<'rv> {
    pub(crate) encoded_len: usize,
    pub(crate) control: StagedControlEmission<'rv>,
}

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel::core) struct SendProgressCommitPlan {
    pub(crate) delta: PreparedCommitDelta,
}

#[path = "runtime_types/commit.rs"]
mod commit;
pub(crate) use commit::*;

pub(crate) struct SendCommitProof<'rv> {
    pub(in crate::endpoint::kernel::core) descriptor: SendDescriptorTerminal<'rv>,
    pub(in crate::endpoint::kernel::core) progress: SendProgressCommitPlan,
}

pub(crate) struct SendCommitPlan<'rv> {
    pub(in crate::endpoint::kernel::core) control: StagedControlEmission<'rv>,
    pub(in crate::endpoint::kernel::core) proof: SendCommitProof<'rv>,
}

impl<'rv> SendCommitPlan<'rv> {
    #[inline(always)]
    pub(in crate::endpoint) fn into_rollback_parts(
        self,
    ) -> (StagedControlEmission<'rv>, SendDescriptorTerminal<'rv>) {
        let Self { control, proof } = self;
        let SendCommitProof {
            descriptor,
            progress: _,
        } = proof;
        (control, descriptor)
    }
}

pub(crate) struct PendingSendIo<'r> {
    pub(in crate::endpoint::kernel) lane_idx: usize,
    pub(in crate::endpoint::kernel) transport: lane_port::PendingSend<'r>,
    pub(in crate::endpoint::kernel) commit_plan: Option<SendCommitPlan<'r>>,
}

impl<'r> PendingSendIo<'r> {
    #[inline(always)]
    pub(in crate::endpoint::kernel) fn lane_idx(&self) -> usize {
        self.lane_idx
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
    pub(crate) descriptor: SendDescriptorPublication<'rv>,
}

#[derive(Clone, Copy)]
pub(crate) struct MsgFlags(u8);

impl MsgFlags {
    const EXPECTS_CONTROL: u8 = 1 << 0;
    const ACCEPTS_EMPTY_PAYLOAD: u8 = 1 << 1;

    #[inline(always)]
    pub(crate) const fn new(expects_control: bool, accepts_empty_payload: bool) -> Self {
        let mut bits = 0u8;
        if expects_control {
            bits |= Self::EXPECTS_CONTROL;
        }
        if accepts_empty_payload {
            bits |= Self::ACCEPTS_EMPTY_PAYLOAD;
        }
        Self(bits)
    }

    #[inline(always)]
    pub(crate) const fn expects_control(self) -> bool {
        self.0 & Self::EXPECTS_CONTROL != 0
    }

    #[inline(always)]
    pub(crate) const fn accepts_empty_payload(self) -> bool {
        self.0 & Self::ACCEPTS_EMPTY_PAYLOAD != 0
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct MsgRuntimeCore {
    logical_label: crate::transport::LogicalLabel,
    frame_label: crate::transport::FrameLabel,
    flags: MsgFlags,
}

impl MsgRuntimeCore {
    #[inline]
    pub(crate) const fn new(
        logical_label: u8,
        frame_label: crate::transport::FrameLabel,
        expects_control: bool,
        accepts_empty_payload: bool,
    ) -> Self {
        Self {
            logical_label: crate::transport::LogicalLabel::new(logical_label),
            frame_label,
            flags: MsgFlags::new(expects_control, accepts_empty_payload),
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
    pub(crate) const fn expects_control(self) -> bool {
        self.flags.expects_control()
    }

    #[inline]
    pub(crate) const fn accepts_empty_payload(self) -> bool {
        self.flags.accepts_empty_payload()
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct RecvRuntimeDesc {
    pub(crate) core: MsgRuntimeCore,
}

impl RecvRuntimeDesc {
    #[inline]
    pub(crate) const fn new(
        logical_label: u8,
        frame_label: crate::transport::FrameLabel,
        expects_control: bool,
        accepts_empty_payload: bool,
    ) -> Self {
        Self {
            core: MsgRuntimeCore::new(
                logical_label,
                frame_label,
                expects_control,
                accepts_empty_payload,
            ),
        }
    }

    #[inline]
    pub(crate) const fn frame_label(self) -> crate::transport::FrameLabel {
        self.core.frame_label()
    }

    #[inline]
    pub(crate) const fn expects_control(self) -> bool {
        self.core.expects_control()
    }

    #[inline]
    pub(crate) const fn accepts_empty_payload(self) -> bool {
        self.core.accepts_empty_payload()
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct DecodeRuntimeDesc {
    pub(crate) core: MsgRuntimeCore,
    validate: for<'a> fn(Payload<'a>) -> Result<(), crate::transport::wire::CodecError>,
    synthetic: for<'a> fn(&'a mut [u8]) -> Result<Payload<'a>, crate::transport::wire::CodecError>,
}

impl DecodeRuntimeDesc {
    #[inline]
    pub(crate) const fn new(
        logical_label: u8,
        frame_label: crate::transport::FrameLabel,
        expects_control: bool,
        validate: for<'a> fn(Payload<'a>) -> Result<(), crate::transport::wire::CodecError>,
        synthetic: for<'a> fn(
            &'a mut [u8],
        ) -> Result<Payload<'a>, crate::transport::wire::CodecError>,
    ) -> Self {
        Self {
            core: MsgRuntimeCore::new(logical_label, frame_label, expects_control, false),
            validate,
            synthetic,
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
    pub(crate) const fn expects_control(self) -> bool {
        self.core.expects_control()
    }

    #[inline]
    pub(crate) fn validate_payload(
        self,
        payload: Payload<'_>,
    ) -> Result<(), crate::transport::wire::CodecError> {
        (self.validate)(payload)
    }

    #[inline]
    pub(crate) fn synthetic_payload<'a>(
        self,
        scratch: &'a mut [u8],
    ) -> Result<Payload<'a>, crate::transport::wire::CodecError> {
        (self.synthetic)(scratch)
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct SendRuntimeDesc {
    pub(crate) core: MsgRuntimeCore,
    pub(crate) control: Option<ControlDesc>,
    encode_payload: crate::transport::wire::ErasedEncoder,
    encode_control_handle: Option<fn(SessionId, Lane, u64) -> [u8; CAP_HANDLE_LEN]>,
}

impl SendRuntimeDesc {
    #[inline]
    pub(crate) const fn new(
        logical_label: u8,
        frame_label: crate::transport::FrameLabel,
        expects_control: bool,
        control: Option<ControlDesc>,
        encode_payload: crate::transport::wire::ErasedEncoder,
        encode_control_handle: Option<fn(SessionId, Lane, u64) -> [u8; CAP_HANDLE_LEN]>,
    ) -> Self {
        Self {
            core: MsgRuntimeCore::new(logical_label, frame_label, expects_control, false),
            control,
            encode_payload,
            encode_control_handle,
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
    pub(crate) const fn expects_control(self) -> bool {
        self.core.expects_control()
    }

    #[inline]
    pub(crate) const fn control(self) -> Option<ControlDesc> {
        self.control
    }

    #[inline]
    pub(crate) const fn encode_control_handle(
        self,
    ) -> Option<fn(SessionId, Lane, u64) -> [u8; CAP_HANDLE_LEN]> {
        self.encode_control_handle
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
    pub(crate) const fn header_bytes(self) -> usize {
        self.header_bytes
    }

    #[inline(always)]
    pub(crate) const fn port_slots_offset(self) -> usize {
        self.port_slots_offset
    }

    #[inline(always)]
    pub(crate) const fn port_slots_bytes(self) -> usize {
        self.port_slots_bytes
    }

    #[inline(always)]
    pub(crate) const fn guard_slots_offset(self) -> usize {
        self.guard_slots_offset
    }

    #[inline(always)]
    pub(crate) const fn guard_slots_bytes(self) -> usize {
        self.guard_slots_bytes
    }

    #[inline(always)]
    pub(crate) const fn arena_offset(self) -> usize {
        self.arena_offset
    }

    #[inline(always)]
    pub(crate) const fn arena_bytes(self) -> usize {
        self.arena_bytes
    }

    #[inline(always)]
    pub(crate) const fn arena_align(self) -> usize {
        self.arena_align
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
    let mask = align.saturating_sub(1);
    (value + mask) & !mask
}

#[inline(always)]
const fn storage_max(lhs: usize, rhs: usize) -> usize {
    if lhs > rhs { lhs } else { rhs }
}

#[inline]
pub(crate) const fn cursor_endpoint_storage_layout<
    'r,
    const ROLE: u8,
    T,
    U,
    C,
    E,
    const MAX_RV: usize,
    Mint,
>(
    arena_layout: &EndpointArenaLayout,
    lane_slot_count: usize,
) -> CursorEndpointStorageLayout
where
    T: Transport + 'r,
    U: LabelUniverse + 'r,
    C: crate::runtime::config::Clock + 'r,
    E: EpochTable + 'r,
    Mint: MintConfigMarker,
{
    let header_bytes = core::mem::size_of::<CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint>>();
    let header_align = core::mem::align_of::<CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint>>();
    let port_slots_align = core::mem::align_of::<Option<Port<'r, T, E>>>();
    let port_slots_bytes =
        core::mem::size_of::<Option<Port<'r, T, E>>>().saturating_mul(lane_slot_count);
    let port_slots_offset = storage_align_up(header_bytes, port_slots_align);
    let guard_slots_align = core::mem::align_of::<Option<LaneGuard<'r, T, U, C>>>();
    let guard_slots_bytes =
        core::mem::size_of::<Option<LaneGuard<'r, T, U, C>>>().saturating_mul(lane_slot_count);
    let guard_slots_offset =
        storage_align_up(port_slots_offset + port_slots_bytes, guard_slots_align);
    let arena_offset = storage_align_up(
        guard_slots_offset + guard_slots_bytes,
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
        total_bytes: arena_offset + arena_layout.total_bytes(),
        total_align,
    }
}

#[cfg(test)]
mod size_tests {
    use super::*;
    use crate::control::cluster::core::DescriptorTerminal;

    #[test]
    fn pending_send_commit_proof_stays_compact() {
        assert!(
            core::mem::size_of::<DescriptorTerminal>() <= 40,
            "DescriptorTerminal grew to {} bytes",
            core::mem::size_of::<DescriptorTerminal>()
        );
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
