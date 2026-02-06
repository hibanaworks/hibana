//! Typed control-plane payloads carried by `Msg<LABEL, Payload>`.
//!
//! These payloads are used for cancel, checkpoint, commit, and rollback
//! operations. They are `no_std` friendly and implement the crate's
//! `WireEncode` / `WireDecode` traits so they can be sent over the same
//! transport as regular session data.

use crate::transport::wire::{CodecError, WireDecode, WireEncode};

/// Explicit cancellation notice propagated through the data plane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CancelNotice {
    /// Application-defined cancellation reason code.
    pub reason: u8,
}

impl CancelNotice {
    pub const WIRE_LEN: usize = 1;

    /// Standard reason code used by examples when rejecting invalid requests.
    pub const REASON_INVALID_REQUEST: u8 = 1;
}

impl WireEncode for CancelNotice {
    fn encoded_len(&self) -> Option<usize> {
        Some(Self::WIRE_LEN)
    }

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < Self::WIRE_LEN {
            return Err(CodecError::Truncated);
        }
        out[0] = self.reason;
        Ok(Self::WIRE_LEN)
    }
}

impl<'a> WireDecode<'a> for CancelNotice {
    fn decode_from(input: &'a [u8]) -> Result<Self, CodecError> {
        if input.len() < Self::WIRE_LEN {
            return Err(CodecError::Truncated);
        }
        if input.len() > Self::WIRE_LEN {
            return Err(CodecError::Invalid("unexpected cancel payload length"));
        }
        Ok(Self { reason: input[0] })
    }
}

/// Checkpoint proposal propagated by the initiator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CheckpointProposal {
    /// Proposed generation number.
    pub generation: u16,
}

impl CheckpointProposal {
    pub const WIRE_LEN: usize = core::mem::size_of::<u16>();
}

impl WireEncode for CheckpointProposal {
    fn encoded_len(&self) -> Option<usize> {
        Some(Self::WIRE_LEN)
    }

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < Self::WIRE_LEN {
            return Err(CodecError::Truncated);
        }
        out[..2].copy_from_slice(&self.generation.to_be_bytes());
        Ok(Self::WIRE_LEN)
    }
}

impl<'a> WireDecode<'a> for CheckpointProposal {
    fn decode_from(input: &'a [u8]) -> Result<Self, CodecError> {
        if input.len() < Self::WIRE_LEN {
            return Err(CodecError::Truncated);
        }
        if input.len() > Self::WIRE_LEN {
            return Err(CodecError::Invalid("unexpected checkpoint payload length"));
        }
        let mut buf = [0u8; 2];
        buf.copy_from_slice(&input[..2]);
        Ok(Self {
            generation: u16::from_be_bytes(buf),
        })
    }
}

/// Ack generated once a checkpoint has been persisted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CheckpointAck {
    pub generation: u16,
}

impl CheckpointAck {
    pub const WIRE_LEN: usize = core::mem::size_of::<u16>();
}

impl WireEncode for CheckpointAck {
    fn encoded_len(&self) -> Option<usize> {
        Some(Self::WIRE_LEN)
    }

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < Self::WIRE_LEN {
            return Err(CodecError::Truncated);
        }
        out[..2].copy_from_slice(&self.generation.to_be_bytes());
        Ok(Self::WIRE_LEN)
    }
}

impl<'a> WireDecode<'a> for CheckpointAck {
    fn decode_from(input: &'a [u8]) -> Result<Self, CodecError> {
        if input.len() < Self::WIRE_LEN {
            return Err(CodecError::Truncated);
        }
        if input.len() > Self::WIRE_LEN {
            return Err(CodecError::Invalid("unexpected checkpoint ack length"));
        }
        let mut buf = [0u8; 2];
        buf.copy_from_slice(&input[..2]);
        Ok(Self {
            generation: u16::from_be_bytes(buf),
        })
    }
}

/// Commit acknowledgement indicating a generation has been persisted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommitAck {
    pub generation: u16,
}

impl CommitAck {
    pub const WIRE_LEN: usize = core::mem::size_of::<u16>();
}

impl WireEncode for CommitAck {
    fn encoded_len(&self) -> Option<usize> {
        Some(Self::WIRE_LEN)
    }

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < Self::WIRE_LEN {
            return Err(CodecError::Truncated);
        }
        out[..2].copy_from_slice(&self.generation.to_be_bytes());
        Ok(Self::WIRE_LEN)
    }
}

impl<'a> WireDecode<'a> for CommitAck {
    fn decode_from(input: &'a [u8]) -> Result<Self, CodecError> {
        if input.len() < Self::WIRE_LEN {
            return Err(CodecError::Truncated);
        }
        if input.len() > Self::WIRE_LEN {
            return Err(CodecError::Invalid("unexpected commit payload length"));
        }
        let mut buf = [0u8; 2];
        buf.copy_from_slice(&input[..2]);
        Ok(Self {
            generation: u16::from_be_bytes(buf),
        })
    }
}

/// Rollback intent propagated when reverting a checkpoint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RollbackIntent {
    pub generation: u16,
    pub reason: u8,
}

impl RollbackIntent {
    pub const WIRE_LEN: usize = core::mem::size_of::<u16>() + 1;

    /// Standard reason code for cooperative rollback.
    pub const REASON_COOPERATIVE: u8 = 1;
}

impl WireEncode for RollbackIntent {
    fn encoded_len(&self) -> Option<usize> {
        Some(Self::WIRE_LEN)
    }

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < Self::WIRE_LEN {
            return Err(CodecError::Truncated);
        }
        out[..2].copy_from_slice(&self.generation.to_be_bytes());
        out[2] = self.reason;
        Ok(Self::WIRE_LEN)
    }
}

impl<'a> WireDecode<'a> for RollbackIntent {
    fn decode_from(input: &'a [u8]) -> Result<Self, CodecError> {
        if input.len() < Self::WIRE_LEN {
            return Err(CodecError::Truncated);
        }
        if input.len() > Self::WIRE_LEN {
            return Err(CodecError::Invalid("unexpected rollback payload length"));
        }
        let mut buf = [0u8; 2];
        buf.copy_from_slice(&input[..2]);
        Ok(Self {
            generation: u16::from_be_bytes(buf),
            reason: input[2],
        })
    }
}

/// Crash notice observed on unreliable links.
///
/// Crash notices are generated by the runtime to indicate that a remote role
/// has stopped participating. They are never sent proactively by application
/// code; instead, they are consumed on the receive side to transition the
/// endpoint typestate into a stop state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CrashNotice {
    /// Role identifier where the crash was observed.
    pub role: u8,
    /// Generation associated with the crashed lane.
    pub generation: u16,
}

impl CrashNotice {
    pub const WIRE_LEN: usize = 3;
}

impl WireEncode for CrashNotice {
    fn encoded_len(&self) -> Option<usize> {
        Some(Self::WIRE_LEN)
    }

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < Self::WIRE_LEN {
            return Err(CodecError::Truncated);
        }
        out[0] = self.role;
        out[1..3].copy_from_slice(&self.generation.to_be_bytes());
        Ok(Self::WIRE_LEN)
    }
}

impl<'a> WireDecode<'a> for CrashNotice {
    fn decode_from(input: &'a [u8]) -> Result<Self, CodecError> {
        if input.len() < Self::WIRE_LEN {
            return Err(CodecError::Truncated);
        }
        if input.len() > Self::WIRE_LEN {
            return Err(CodecError::Invalid("unexpected crash payload length"));
        }
        let mut buf = [0u8; 2];
        buf.copy_from_slice(&input[1..3]);
        Ok(Self {
            role: input[0],
            generation: u16::from_be_bytes(buf),
        })
    }
}
