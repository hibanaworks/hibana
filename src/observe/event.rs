//! Canonical 20-byte tap event and semantic Evidence decode.

use crate::{
    observe::ids,
    transport::wire::{CodecError, Payload, WireEncode, WirePayload, require_exact_len},
};

/// 20-byte tap record with causal key tracking for roll-π reversibility.
///
/// Layout: `ts32, id16, causal_key16, arg0_32, arg1_32, arg2_32`
/// - `ts`: Timestamp (monotonic counter or wall-clock tick)
/// - `id`: Event identifier (from `crate::observe::ids::*`)
/// - `causal_key`: Causal key for reversible rollback tracking (roll-π)
///   - High 8 bits: role/lane index
///   - Low 8 bits: sequence number within epoch
/// - `arg0`, `arg1`: Context-dependent arguments (sid, gen, label, etc.)
/// - `arg2`: Extended context (e.g., ScopeId range/nest ordinals)
///
/// **Future extension**: For roll-π memory tracking, `causal_key` encodes
/// the (role, seq) pair that establishes causal dependencies. Rollback
/// operations can reconstruct causal history by following these keys.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct TapEvent {
    pub ts: u32,
    pub id: u16,
    pub causal_key: u16,
    pub arg0: u32,
    pub arg1: u32,
    pub arg2: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Evidence {
    kind: u16,
    reason: u8,
    input: [u32; 4],
}

impl Evidence {
    #[inline]
    pub const fn kind(self) -> u16 {
        self.kind
    }

    #[inline]
    pub const fn reason(self) -> u8 {
        self.reason
    }

    #[inline]
    pub const fn input(self) -> [u32; 4] {
        self.input
    }

    #[inline]
    pub const fn input_word(self, index: usize) -> Option<u32> {
        if index < self.input.len() {
            Some(self.input[index])
        } else {
            None
        }
    }
}

impl TapEvent {
    #[inline]
    pub const fn with_arg0(mut self, arg0: u32) -> Self {
        self.arg0 = arg0;
        self
    }

    #[inline]
    pub const fn with_arg1(mut self, arg1: u32) -> Self {
        self.arg1 = arg1;
        self
    }

    #[inline]
    pub const fn with_arg2(mut self, arg2: u32) -> Self {
        self.arg2 = arg2;
        self
    }

    #[inline]
    pub const fn with_causal_key(mut self, causal_key: u16) -> Self {
        self.causal_key = causal_key;
        self
    }

    /// Extract role/lane from causal key (high 8 bits).
    #[inline]
    pub const fn causal_role(self) -> u8 {
        (self.causal_key >> 8) as u8
    }

    /// Extract sequence number from causal key (low 8 bits).
    #[inline]
    pub const fn causal_seq(self) -> u8 {
        (self.causal_key & 0xFF) as u8
    }

    /// Construct causal key from role and sequence.
    #[inline]
    pub const fn make_causal_key(role: u8, seq: u8) -> u16 {
        ((role as u16) << 8) | (seq as u16)
    }

    /// Create a zeroed event (for array initialization).
    #[inline]
    pub const fn zero() -> Self {
        Self {
            ts: 0,
            id: 0,
            causal_key: 0,
            arg0: 0,
            arg1: 0,
            arg2: 0,
        }
    }

    #[inline]
    pub const fn evidence(self) -> Evidence {
        if self.id == ids::TRANSPORT_MISMATCH {
            let reason = self.causal_seq();
            let expected_lane = self.causal_role() as u32;
            Evidence {
                kind: self.id,
                reason,
                input: [
                    self.arg0,
                    self.arg1,
                    self.arg2,
                    ((self.id as u32) << 16) | (expected_lane << 8) | (reason as u32),
                ],
            }
        } else if self.id == ids::TRANSPORT_FAULT {
            let reason = self.causal_seq();
            let lane = self.causal_role() as u32;
            Evidence {
                kind: self.id,
                reason,
                input: [
                    self.arg0,
                    self.arg1,
                    self.arg2,
                    ((self.id as u32) << 16) | (lane << 8) | (reason as u32),
                ],
            }
        } else {
            Evidence {
                kind: self.id,
                reason: 0,
                input: [self.arg0, self.arg1, self.arg2, self.causal_key as u32],
            }
        }
    }
}

impl WireEncode for TapEvent {
    fn encoded_len(&self) -> Option<usize> {
        Some(20)
    }

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < 20 {
            return Err(CodecError::Truncated);
        }
        out[0..4].copy_from_slice(&self.ts.to_be_bytes());
        out[4..6].copy_from_slice(&self.id.to_be_bytes());
        out[6..8].copy_from_slice(&self.causal_key.to_be_bytes());
        out[8..12].copy_from_slice(&self.arg0.to_be_bytes());
        out[12..16].copy_from_slice(&self.arg1.to_be_bytes());
        out[16..20].copy_from_slice(&self.arg2.to_be_bytes());
        Ok(20)
    }
}

impl WirePayload for TapEvent {
    type Decoded<'a> = Self;

    fn validate_payload(input: Payload<'_>) -> Result<(), CodecError> {
        require_exact_len(input.as_bytes().len(), 20, "payload length")
    }

    fn decode_validated_payload<'a>(input: Payload<'a>) -> Self::Decoded<'a> {
        let bytes = input.as_bytes();
        Self {
            ts: u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            id: u16::from_be_bytes([bytes[4], bytes[5]]),
            causal_key: u16::from_be_bytes([bytes[6], bytes[7]]),
            arg0: u32::from_be_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
            arg1: u32::from_be_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]),
            arg2: u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::TapEvent;
    use crate::observe::ids;

    #[test]
    fn transport_mismatch_evidence_carries_expected_lane_and_reason() {
        let event = TapEvent::zero()
            .with_causal_key(TapEvent::make_causal_key(
                1,
                ids::TRANSPORT_MISMATCH_SESSION,
            ))
            .with_arg0(0x1111_2222)
            .with_arg1(0x3333_4444)
            .with_arg2(0x0102_0304);
        let event = TapEvent {
            id: ids::TRANSPORT_MISMATCH,
            ..event
        };
        let evidence = event.evidence();
        assert_eq!(evidence.kind(), ids::TRANSPORT_MISMATCH);
        assert_eq!(evidence.reason(), ids::TRANSPORT_MISMATCH_SESSION);
        assert_eq!(
            evidence.input(),
            [
                0x1111_2222,
                0x3333_4444,
                0x0102_0304,
                ((ids::TRANSPORT_MISMATCH as u32) << 16)
                    | (1 << 8)
                    | ids::TRANSPORT_MISMATCH_SESSION as u32
            ]
        );
    }

    #[test]
    fn transport_fault_evidence_carries_lane_and_reason() {
        let event = TapEvent::zero()
            .with_causal_key(TapEvent::make_causal_key(2, ids::TRANSPORT_FAULT_DEADLINE))
            .with_arg0(0xaaaa_bbbb);
        let event = TapEvent {
            id: ids::TRANSPORT_FAULT,
            ..event
        };
        let evidence = event.evidence();
        assert_eq!(evidence.kind(), ids::TRANSPORT_FAULT);
        assert_eq!(evidence.reason(), ids::TRANSPORT_FAULT_DEADLINE);
        assert_eq!(
            evidence.input(),
            [
                0xaaaa_bbbb,
                0,
                0,
                ((ids::TRANSPORT_FAULT as u32) << 16)
                    | (2 << 8)
                    | ids::TRANSPORT_FAULT_DEADLINE as u32
            ]
        );
    }
}
