use crate::{
    observe::core::TapEvent,
    substrate::policy::epf::Slot,
    transport::wire::{CodecError, WireDecode, WireEncode},
};

#[cfg(test)]
use crate::epf::{host::HostError, loader::LoaderError, verifier::VerifyError};

pub(crate) const LOAD_CHUNK_MAX: usize = 1024;
const TAP_EVENT_WIRE_LEN: usize = 20;

/// Errors that can occur during the management session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MgmtError {
    InvalidSlot(u8),
    InvalidTransition,
    ChunkOutOfOrder { expected: u32, got: u32 },
    ChunkTooLarge { remaining: u32, provided: u32 },
    LoaderNotFinalised,
    NoStagedImage,
    NoActiveImage,
    NoPreviousImage,
    CapabilityMismatch,
    ObserveUnavailable,
    HostInstallFailed,
    HostUninstallFailed,
    StreamEnded,
}

#[cfg(test)]
impl From<LoaderError> for MgmtError {
    fn from(err: LoaderError) -> Self {
        match err {
            LoaderError::AlreadyLoading => MgmtError::InvalidTransition,
            LoaderError::NotLoading => MgmtError::InvalidTransition,
            LoaderError::CodeTooLarge { declared } => MgmtError::ChunkTooLarge {
                remaining: 0,
                provided: declared as u32,
            },
            LoaderError::UnexpectedOffset { expected, got } => {
                MgmtError::ChunkOutOfOrder { expected, got }
            }
            LoaderError::ChunkTooLarge {
                remaining,
                provided,
            } => MgmtError::ChunkTooLarge {
                remaining,
                provided,
            },
            LoaderError::HashMismatch { .. } => MgmtError::LoaderNotFinalised,
            LoaderError::Verify(err) => MgmtError::from(err),
        }
    }
}

#[cfg(test)]
impl From<VerifyError> for MgmtError {
    fn from(_: VerifyError) -> Self {
        MgmtError::LoaderNotFinalised
    }
}

#[cfg(test)]
impl From<HostError> for MgmtError {
    fn from(err: HostError) -> Self {
        match err {
            HostError::SlotOccupied => MgmtError::InvalidTransition,
            HostError::SlotEmpty => MgmtError::HostUninstallFailed,
            HostError::InvalidFuel => MgmtError::HostInstallFailed,
            HostError::ScratchTooSmall { .. } => MgmtError::HostInstallFailed,
            HostError::ScratchTooLarge { .. } => MgmtError::HostInstallFailed,
        }
    }
}

impl WireEncode for MgmtError {
    fn encoded_len(&self) -> Option<usize> {
        Some(match self {
            MgmtError::InvalidSlot(_) => 2,
            MgmtError::ChunkOutOfOrder { .. } => 9,
            MgmtError::ChunkTooLarge { .. } => 9,
            _ => 1,
        })
    }

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        let need = self.encoded_len().unwrap_or(1);
        if out.len() < need {
            return Err(CodecError::Truncated);
        }
        match self {
            MgmtError::InvalidSlot(slot) => {
                out[0] = 0;
                out[1] = *slot;
            }
            MgmtError::InvalidTransition => out[0] = 1,
            MgmtError::ChunkOutOfOrder { expected, got } => {
                out[0] = 2;
                out[1..5].copy_from_slice(&expected.to_be_bytes());
                out[5..9].copy_from_slice(&got.to_be_bytes());
            }
            MgmtError::ChunkTooLarge {
                remaining,
                provided,
            } => {
                out[0] = 3;
                out[1..5].copy_from_slice(&remaining.to_be_bytes());
                out[5..9].copy_from_slice(&provided.to_be_bytes());
            }
            MgmtError::LoaderNotFinalised => out[0] = 4,
            MgmtError::NoStagedImage => out[0] = 5,
            MgmtError::NoActiveImage => out[0] = 6,
            MgmtError::NoPreviousImage => out[0] = 7,
            MgmtError::CapabilityMismatch => out[0] = 8,
            MgmtError::ObserveUnavailable => out[0] = 9,
            MgmtError::HostInstallFailed => out[0] = 10,
            MgmtError::HostUninstallFailed => out[0] = 11,
            MgmtError::StreamEnded => out[0] = 12,
        }
        Ok(need)
    }
}

impl<'a> WireDecode<'a> for MgmtError {
    fn decode_from(input: &'a [u8]) -> Result<Self, CodecError> {
        if input.is_empty() {
            return Err(CodecError::Truncated);
        }
        match input[0] {
            0 => {
                if input.len() < 2 {
                    return Err(CodecError::Truncated);
                }
                Ok(MgmtError::InvalidSlot(input[1]))
            }
            1 => Ok(MgmtError::InvalidTransition),
            2 => {
                if input.len() < 9 {
                    return Err(CodecError::Truncated);
                }
                Ok(MgmtError::ChunkOutOfOrder {
                    expected: u32::from_be_bytes([input[1], input[2], input[3], input[4]]),
                    got: u32::from_be_bytes([input[5], input[6], input[7], input[8]]),
                })
            }
            3 => {
                if input.len() < 9 {
                    return Err(CodecError::Truncated);
                }
                Ok(MgmtError::ChunkTooLarge {
                    remaining: u32::from_be_bytes([input[1], input[2], input[3], input[4]]),
                    provided: u32::from_be_bytes([input[5], input[6], input[7], input[8]]),
                })
            }
            4 => Ok(MgmtError::LoaderNotFinalised),
            5 => Ok(MgmtError::NoStagedImage),
            6 => Ok(MgmtError::NoActiveImage),
            7 => Ok(MgmtError::NoPreviousImage),
            8 => Ok(MgmtError::CapabilityMismatch),
            9 => Ok(MgmtError::ObserveUnavailable),
            10 => Ok(MgmtError::HostInstallFailed),
            11 => Ok(MgmtError::HostUninstallFailed),
            12 => Ok(MgmtError::StreamEnded),
            _ => Err(CodecError::Invalid("unknown management error tag")),
        }
    }
}

/// Typical management protocol replies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reply {
    Loaded(LoadReport),
    ActivationScheduled(TransitionReport),
    Reverted(TransitionReport),
    Stats {
        stats: StatsResp,
        staged_version: Option<u32>,
    },
}

/// One-shot report returned when an image is staged but not activated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoadReport {
    pub staged_version: u32,
}

/// Payload carried by the stats reply route.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatsReply {
    pub stats: StatsResp,
    pub staged_version: Option<u32>,
}

/// Request payload for code upload branches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoadRequest<'a> {
    pub slot: crate::substrate::policy::epf::Slot,
    pub code: &'a [u8],
    pub fuel_max: u16,
    pub mem_len: u16,
}

/// Request payload for slot-scoped command branches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlotRequest {
    pub slot: crate::substrate::policy::epf::Slot,
}

/// Management requests carried by the request/reply management session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request<'a> {
    Load(LoadRequest<'a>),
    LoadAndActivate(LoadRequest<'a>),
    Activate(SlotRequest),
    Revert(SlotRequest),
    Stats(SlotRequest),
}

/// Slot-level metrics.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StatsResp {
    pub traps: u32,
    pub aborts: u32,
    pub fuel_used: u32,
    pub active_version: u32,
}

/// Policy statistics collected during transitions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TransitionReport {
    pub version: u32,
    pub policy_stats: PolicyStats,
}

/// Policy event statistics harvested from tap events.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PolicyStats {
    pub aborts: u32,
    pub traps: u32,
    pub annotations: u32,
    pub effects: u32,
    pub effects_ok: u32,
    pub commits: u32,
    pub rollbacks: u32,
    pub last_commit: Option<u32>,
    pub last_rollback: Option<u32>,
}

/// Payload carried by the `LoadBegin` message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoadBegin {
    pub slot: crate::substrate::policy::epf::Slot,
    pub code_len: u32,
    pub fuel_max: u16,
    pub mem_len: u16,
    pub hash: u32,
}

/// Payload for `LoadChunk`; the chunk body lives in a fixed-size buffer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadChunk {
    pub offset: u32,
    pub len: u16,
    pub bytes: [u8; LOAD_CHUNK_MAX],
}

impl LoadChunk {
    pub fn new(offset: u32, chunk: &[u8]) -> Self {
        assert!(
            chunk.len() <= LOAD_CHUNK_MAX,
            "chunk length exceeds management chunk capacity"
        );
        let mut bytes = [0u8; LOAD_CHUNK_MAX];
        bytes[..chunk.len()].copy_from_slice(chunk);
        Self {
            offset,
            len: chunk.len() as u16,
            bytes,
        }
    }

    #[inline]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes[..self.len as usize]
    }
}

/// Subscribe request for streaming observe.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SubscribeReq {
    pub flags: u16,
}

impl WireEncode for SubscribeReq {
    fn encoded_len(&self) -> Option<usize> {
        Some(2)
    }

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < 2 {
            return Err(CodecError::Truncated);
        }
        out[0..2].copy_from_slice(&self.flags.to_be_bytes());
        Ok(2)
    }
}

impl<'a> WireDecode<'a> for SubscribeReq {
    fn decode_from(input: &'a [u8]) -> Result<Self, CodecError> {
        if input.len() < 2 {
            return Err(CodecError::Truncated);
        }
        let flags = u16::from_be_bytes([input[0], input[1]]);
        Ok(SubscribeReq { flags })
    }
}

impl WireEncode for TapEvent {
    fn encoded_len(&self) -> Option<usize> {
        Some(TAP_EVENT_WIRE_LEN)
    }

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < TAP_EVENT_WIRE_LEN {
            return Err(CodecError::Truncated);
        }
        out[0..4].copy_from_slice(&self.ts.to_be_bytes());
        out[4..6].copy_from_slice(&self.id.to_be_bytes());
        out[6..8].copy_from_slice(&self.causal_key.to_be_bytes());
        out[8..12].copy_from_slice(&self.arg0.to_be_bytes());
        out[12..16].copy_from_slice(&self.arg1.to_be_bytes());
        out[16..20].copy_from_slice(&self.arg2.to_be_bytes());
        Ok(TAP_EVENT_WIRE_LEN)
    }
}

impl<'a> WireDecode<'a> for TapEvent {
    fn decode_from(input: &'a [u8]) -> Result<Self, CodecError> {
        if input.len() < TAP_EVENT_WIRE_LEN {
            return Err(CodecError::Truncated);
        }
        Ok(TapEvent {
            ts: u32::from_be_bytes([input[0], input[1], input[2], input[3]]),
            id: u16::from_be_bytes([input[4], input[5]]),
            causal_key: u16::from_be_bytes([input[6], input[7]]),
            arg0: u32::from_be_bytes([input[8], input[9], input[10], input[11]]),
            arg1: u32::from_be_bytes([input[12], input[13], input[14], input[15]]),
            arg2: u32::from_be_bytes([input[16], input[17], input[18], input[19]]),
        })
    }
}

impl WireEncode for StatsResp {
    fn encoded_len(&self) -> Option<usize> {
        Some(16)
    }

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < 16 {
            return Err(CodecError::Truncated);
        }
        out[0..4].copy_from_slice(&self.traps.to_be_bytes());
        out[4..8].copy_from_slice(&self.aborts.to_be_bytes());
        out[8..12].copy_from_slice(&self.fuel_used.to_be_bytes());
        out[12..16].copy_from_slice(&self.active_version.to_be_bytes());
        Ok(16)
    }
}

impl<'a> WireDecode<'a> for StatsResp {
    fn decode_from(input: &'a [u8]) -> Result<Self, CodecError> {
        if input.len() < 16 {
            return Err(CodecError::Truncated);
        }
        Ok(StatsResp {
            traps: u32::from_be_bytes([input[0], input[1], input[2], input[3]]),
            aborts: u32::from_be_bytes([input[4], input[5], input[6], input[7]]),
            fuel_used: u32::from_be_bytes([input[8], input[9], input[10], input[11]]),
            active_version: u32::from_be_bytes([input[12], input[13], input[14], input[15]]),
        })
    }
}

impl WireEncode for PolicyStats {
    fn encoded_len(&self) -> Option<usize> {
        Some(38)
    }

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < 38 {
            return Err(CodecError::Truncated);
        }
        out[0..4].copy_from_slice(&self.aborts.to_be_bytes());
        out[4..8].copy_from_slice(&self.traps.to_be_bytes());
        out[8..12].copy_from_slice(&self.annotations.to_be_bytes());
        out[12..16].copy_from_slice(&self.effects.to_be_bytes());
        out[16..20].copy_from_slice(&self.effects_ok.to_be_bytes());
        out[20..24].copy_from_slice(&self.commits.to_be_bytes());
        out[24..28].copy_from_slice(&self.rollbacks.to_be_bytes());
        out[28..32].copy_from_slice(&self.last_commit.unwrap_or(0).to_be_bytes());
        out[32] = u8::from(self.last_commit.is_some());
        out[33..37].copy_from_slice(&self.last_rollback.unwrap_or(0).to_be_bytes());
        out[37] = u8::from(self.last_rollback.is_some());
        Ok(38)
    }
}

impl<'a> WireDecode<'a> for PolicyStats {
    fn decode_from(input: &'a [u8]) -> Result<Self, CodecError> {
        if input.len() < 38 {
            return Err(CodecError::Truncated);
        }
        let last_commit = u32::from_be_bytes([input[28], input[29], input[30], input[31]]);
        let last_rollback = u32::from_be_bytes([input[33], input[34], input[35], input[36]]);
        Ok(PolicyStats {
            aborts: u32::from_be_bytes([input[0], input[1], input[2], input[3]]),
            traps: u32::from_be_bytes([input[4], input[5], input[6], input[7]]),
            annotations: u32::from_be_bytes([input[8], input[9], input[10], input[11]]),
            effects: u32::from_be_bytes([input[12], input[13], input[14], input[15]]),
            effects_ok: u32::from_be_bytes([input[16], input[17], input[18], input[19]]),
            commits: u32::from_be_bytes([input[20], input[21], input[22], input[23]]),
            rollbacks: u32::from_be_bytes([input[24], input[25], input[26], input[27]]),
            last_commit: if input[32] == 0 {
                None
            } else {
                Some(last_commit)
            },
            last_rollback: if input[37] == 0 {
                None
            } else {
                Some(last_rollback)
            },
        })
    }
}

impl WireEncode for TransitionReport {
    fn encoded_len(&self) -> Option<usize> {
        Some(42)
    }

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < 42 {
            return Err(CodecError::Truncated);
        }
        out[0..4].copy_from_slice(&self.version.to_be_bytes());
        self.policy_stats.encode_into(&mut out[4..])?;
        Ok(42)
    }
}

impl<'a> WireDecode<'a> for TransitionReport {
    fn decode_from(input: &'a [u8]) -> Result<Self, CodecError> {
        if input.len() < 42 {
            return Err(CodecError::Truncated);
        }
        Ok(TransitionReport {
            version: u32::from_be_bytes([input[0], input[1], input[2], input[3]]),
            policy_stats: PolicyStats::decode_from(&input[4..42])?,
        })
    }
}

impl WireEncode for LoadReport {
    fn encoded_len(&self) -> Option<usize> {
        Some(4)
    }

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < 4 {
            return Err(CodecError::Truncated);
        }
        out[0..4].copy_from_slice(&self.staged_version.to_be_bytes());
        Ok(4)
    }
}

impl<'a> WireDecode<'a> for LoadReport {
    fn decode_from(input: &'a [u8]) -> Result<Self, CodecError> {
        if input.len() < 4 {
            return Err(CodecError::Truncated);
        }
        Ok(LoadReport {
            staged_version: u32::from_be_bytes([input[0], input[1], input[2], input[3]]),
        })
    }
}

impl WireEncode for StatsReply {
    fn encoded_len(&self) -> Option<usize> {
        Some(21)
    }

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < 21 {
            return Err(CodecError::Truncated);
        }
        self.stats.encode_into(out)?;
        out[16] = u8::from(self.staged_version.is_some());
        out[17..21].copy_from_slice(&self.staged_version.unwrap_or(0).to_be_bytes());
        Ok(21)
    }
}

impl<'a> WireDecode<'a> for StatsReply {
    fn decode_from(input: &'a [u8]) -> Result<Self, CodecError> {
        if input.len() < 21 {
            return Err(CodecError::Truncated);
        }
        let stats = StatsResp::decode_from(&input[..16])?;
        let staged_version = if input[16] == 0 {
            None
        } else {
            Some(u32::from_be_bytes([
                input[17], input[18], input[19], input[20],
            ]))
        };
        Ok(StatsReply {
            stats,
            staged_version,
        })
    }
}

impl WireEncode for LoadBegin {
    fn encoded_len(&self) -> Option<usize> {
        Some(13)
    }

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < 13 {
            return Err(CodecError::Truncated);
        }
        out[0] = slot_id(self.slot) as u8;
        out[1..5].copy_from_slice(&self.code_len.to_be_bytes());
        out[5..7].copy_from_slice(&self.fuel_max.to_be_bytes());
        out[7..9].copy_from_slice(&self.mem_len.to_be_bytes());
        out[9..13].copy_from_slice(&self.hash.to_be_bytes());
        Ok(13)
    }
}

impl<'a> WireDecode<'a> for LoadBegin {
    fn decode_from(input: &'a [u8]) -> Result<Self, CodecError> {
        if input.len() < 13 {
            return Err(CodecError::Truncated);
        }
        let slot = decode_slot(input[0])?;
        let code_len = u32::from_be_bytes([input[1], input[2], input[3], input[4]]);
        let fuel_max = u16::from_be_bytes([input[5], input[6]]);
        let mem_len = u16::from_be_bytes([input[7], input[8]]);
        let hash = u32::from_be_bytes([input[9], input[10], input[11], input[12]]);
        Ok(LoadBegin {
            slot,
            code_len,
            fuel_max,
            mem_len,
            hash,
        })
    }
}

impl WireEncode for LoadChunk {
    fn encoded_len(&self) -> Option<usize> {
        Some(6 + self.len as usize)
    }

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        let len = self.len as usize;
        if len > LOAD_CHUNK_MAX {
            return Err(CodecError::Invalid("chunk length exceeds LOAD_CHUNK_MAX"));
        }
        let total = 6 + len;
        if out.len() < total {
            return Err(CodecError::Truncated);
        }
        out[..4].copy_from_slice(&self.offset.to_be_bytes());
        out[4..6].copy_from_slice(&self.len.to_be_bytes());
        out[6..total].copy_from_slice(&self.bytes[..len]);
        Ok(total)
    }
}

impl<'a> WireDecode<'a> for LoadChunk {
    fn decode_from(input: &'a [u8]) -> Result<Self, CodecError> {
        if input.len() < 6 {
            return Err(CodecError::Truncated);
        }
        let offset = u32::from_be_bytes([input[0], input[1], input[2], input[3]]);
        let len = u16::from_be_bytes([input[4], input[5]]);
        let len_usize = len as usize;
        if len_usize > LOAD_CHUNK_MAX {
            return Err(CodecError::Invalid("chunk length exceeds LOAD_CHUNK_MAX"));
        }
        if input.len() < 6 + len_usize {
            return Err(CodecError::Truncated);
        }
        let mut bytes = [0u8; LOAD_CHUNK_MAX];
        bytes[..len_usize].copy_from_slice(&input[6..6 + len_usize]);
        Ok(LoadChunk { offset, len, bytes })
    }
}

impl WireEncode for SlotRequest {
    fn encoded_len(&self) -> Option<usize> {
        Some(1)
    }

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.is_empty() {
            return Err(CodecError::Truncated);
        }
        out[0] = slot_id(self.slot) as u8;
        Ok(1)
    }
}

impl<'a> WireDecode<'a> for SlotRequest {
    fn decode_from(input: &'a [u8]) -> Result<Self, CodecError> {
        if input.is_empty() {
            return Err(CodecError::Truncated);
        }
        Ok(SlotRequest {
            slot: decode_slot(input[0])?,
        })
    }
}

pub(crate) fn slot_id(slot: Slot) -> u32 {
    match slot {
        Slot::Forward => 0,
        Slot::EndpointRx => 1,
        Slot::EndpointTx => 2,
        Slot::Rendezvous => 3,
        Slot::Route => 4,
    }
}

pub(crate) fn decode_slot(slot: u8) -> Result<Slot, CodecError> {
    match slot {
        0 => Ok(Slot::Forward),
        1 => Ok(Slot::EndpointRx),
        2 => Ok(Slot::EndpointTx),
        3 => Ok(Slot::Rendezvous),
        4 => Ok(Slot::Route),
        _ => Err(CodecError::Invalid("unknown management slot")),
    }
}
