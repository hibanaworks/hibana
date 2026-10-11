//! Canonical 16-byte tap event and semantic Evidence decode.

use crate::observe::ids;

/// Opaque 16-byte tap record with a compact causal key for evidence correlation.
///
/// Layout: `ts32, id16, causal_key16, arg0_32, arg1_32`
/// - `ts`: Timestamp (monotonic counter or wall-clock tick)
/// - `id`: Event identifier (from `crate::observe::ids::*`)
/// - `causal_key`: Causal key for reversible evidence correlation
///   - High 8 bits: role or lane discriminator
///   - Low 8 bits: event-specific sequence, result, or reason code
/// - `arg0`, `arg1`: Context-dependent diagnostic words.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct TapEvent {
    bytes: [u8; 16],
}

/// Resident tap representation. The timestamp is the ring ordinal and is
/// reconstructed when a reader materializes the public event.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub(crate) struct TapRecord {
    bytes: [u8; Self::BYTE_LEN],
}

impl TapRecord {
    pub(crate) const BYTE_LEN: usize = 12;

    #[inline(always)]
    pub(crate) const fn from_event(event: TapEvent) -> Self {
        Self {
            bytes: [
                (event.id() >> 8) as u8,
                event.id() as u8,
                (event.causal_key() >> 8) as u8,
                event.causal_key() as u8,
                (event.arg0() >> 24) as u8,
                (event.arg0() >> 16) as u8,
                (event.arg0() >> 8) as u8,
                event.arg0() as u8,
                (event.arg1() >> 24) as u8,
                (event.arg1() >> 16) as u8,
                (event.arg1() >> 8) as u8,
                event.arg1() as u8,
            ],
        }
    }

    #[inline(always)]
    pub(crate) const fn to_event(self, timestamp: u32) -> TapEvent {
        TapEvent::new(
            timestamp,
            u16::from_be_bytes([self.bytes[0], self.bytes[1]]),
            u16::from_be_bytes([self.bytes[2], self.bytes[3]]),
            u32::from_be_bytes([self.bytes[4], self.bytes[5], self.bytes[6], self.bytes[7]]),
            u32::from_be_bytes([self.bytes[8], self.bytes[9], self.bytes[10], self.bytes[11]]),
        )
    }

    #[inline(always)]
    pub(crate) const fn zero() -> Self {
        Self {
            bytes: [0; Self::BYTE_LEN],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
}

impl core::fmt::Debug for TapEvent {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let event = *self;
        f.debug_struct("TapEvent")
            .field("ts", &event.ts())
            .field("id", &event.id())
            .field("causal_key", &event.causal_key())
            .field("arg0", &event.arg0())
            .field("arg1", &event.arg1())
            .field("evidence", &event.evidence())
            .finish()
    }
}

impl TapEvent {
    #[inline]
    pub(crate) const fn new(ts: u32, id: u16, causal_key: u16, arg0: u32, arg1: u32) -> Self {
        Self {
            bytes: [
                (ts >> 24) as u8,
                (ts >> 16) as u8,
                (ts >> 8) as u8,
                ts as u8,
                (id >> 8) as u8,
                id as u8,
                (causal_key >> 8) as u8,
                causal_key as u8,
                (arg0 >> 24) as u8,
                (arg0 >> 16) as u8,
                (arg0 >> 8) as u8,
                arg0 as u8,
                (arg1 >> 24) as u8,
                (arg1 >> 16) as u8,
                (arg1 >> 8) as u8,
                arg1 as u8,
            ],
        }
    }

    #[inline]
    pub const fn ts(self) -> u32 {
        u32::from_be_bytes([self.bytes[0], self.bytes[1], self.bytes[2], self.bytes[3]])
    }

    #[inline]
    pub const fn id(self) -> u16 {
        u16::from_be_bytes([self.bytes[4], self.bytes[5]])
    }

    #[inline]
    pub const fn causal_key(self) -> u16 {
        u16::from_be_bytes([self.bytes[6], self.bytes[7]])
    }

    #[inline]
    pub const fn arg0(self) -> u32 {
        u32::from_be_bytes([self.bytes[8], self.bytes[9], self.bytes[10], self.bytes[11]])
    }

    #[inline]
    pub const fn arg1(self) -> u32 {
        u32::from_be_bytes([
            self.bytes[12],
            self.bytes[13],
            self.bytes[14],
            self.bytes[15],
        ])
    }

    #[inline]
    pub(crate) const fn with_arg0(mut self, arg0: u32) -> Self {
        self.bytes[8] = (arg0 >> 24) as u8;
        self.bytes[9] = (arg0 >> 16) as u8;
        self.bytes[10] = (arg0 >> 8) as u8;
        self.bytes[11] = arg0 as u8;
        self
    }

    #[inline]
    pub(crate) const fn with_arg1(mut self, arg1: u32) -> Self {
        self.bytes[12] = (arg1 >> 24) as u8;
        self.bytes[13] = (arg1 >> 16) as u8;
        self.bytes[14] = (arg1 >> 8) as u8;
        self.bytes[15] = arg1 as u8;
        self
    }

    #[inline]
    pub(crate) const fn with_causal_key(mut self, causal_key: u16) -> Self {
        self.bytes[6] = (causal_key >> 8) as u8;
        self.bytes[7] = causal_key as u8;
        self
    }

    #[inline]
    const fn causal_role(self) -> u8 {
        (self.causal_key() >> 8) as u8
    }

    #[inline]
    const fn causal_seq(self) -> u8 {
        (self.causal_key() & 0xFF) as u8
    }

    /// Construct causal key from role and sequence.
    #[inline]
    pub(crate) const fn make_causal_key(role: u8, seq: u8) -> u16 {
        ((role as u16) << 8) | (seq as u16)
    }

    #[inline]
    pub const fn evidence(self) -> Evidence {
        let id = self.id();
        if id == ids::TRANSPORT_FRAME {
            Evidence {
                kind: id,
                reason: 0,
                input: [self.arg0(), self.arg1(), 0, (id as u32) << 16],
            }
        } else if id == ids::TRANSPORT_MISMATCH || id == ids::TRANSPORT_FAULT {
            self.transport_evidence()
        } else {
            Evidence {
                kind: id,
                reason: 0,
                input: [self.arg0(), self.arg1(), 0, self.causal_key() as u32],
            }
        }
    }

    #[inline]
    const fn transport_evidence(self) -> Evidence {
        let reason = self.causal_seq();
        let lane = self.causal_role() as u32;
        let id = self.id();
        Evidence {
            kind: id,
            reason,
            input: [
                self.arg0(),
                self.arg1(),
                0,
                ((id as u32) << 16) | (lane << 8) | (reason as u32),
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{TapEvent, TapRecord};
    use crate::observe::ids;
    use std::format;

    #[test]
    fn transport_mismatch_evidence_carries_expected_lane_and_reason() {
        let event = super::super::events::raw_event(ids::TRANSPORT_MISMATCH)
            .with_causal_key(TapEvent::make_causal_key(
                1,
                ids::TRANSPORT_MISMATCH_SESSION,
            ))
            .with_arg0(0x1111_2222)
            .with_arg1(0x3333_4444);
        let evidence = event.evidence();
        assert_eq!(evidence.kind(), ids::TRANSPORT_MISMATCH);
        assert_eq!(evidence.reason(), ids::TRANSPORT_MISMATCH_SESSION);
        assert_eq!(
            evidence.input(),
            [
                0x1111_2222,
                0x3333_4444,
                0,
                ((ids::TRANSPORT_MISMATCH as u32) << 16)
                    | (1 << 8)
                    | ids::TRANSPORT_MISMATCH_SESSION as u32
            ]
        );
    }

    #[test]
    fn transport_fault_evidence_carries_lane_and_reason() {
        let event = super::super::events::raw_event(ids::TRANSPORT_FAULT)
            .with_causal_key(TapEvent::make_causal_key(2, ids::TRANSPORT_FAULT_DEADLINE))
            .with_arg0(0xaaaa_bbbb);
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

    #[test]
    fn tap_event_is_exactly_sixteen_bytes() {
        assert_eq!(core::mem::size_of::<TapEvent>(), 16);
        assert_eq!(core::mem::align_of::<TapEvent>(), 1);
    }

    #[test]
    fn resident_tap_record_erases_and_reconstructs_timestamp() {
        let event = TapEvent::new(99, ids::TRANSPORT_FRAME, 0, 0, 0)
            .with_causal_key(0x0506)
            .with_arg0(0x0708_090a)
            .with_arg1(0x0b0c_0d0e);
        let reconstructed = TapRecord::from_event(event).to_event(17);

        assert_eq!(core::mem::size_of::<TapRecord>(), 12);
        assert_eq!(reconstructed.ts(), 17);
        assert_eq!(reconstructed.id(), event.id());
        assert_eq!(reconstructed.causal_key(), event.causal_key());
        assert_eq!(reconstructed.arg0(), event.arg0());
        assert_eq!(reconstructed.arg1(), event.arg1());
    }

    #[test]
    fn tap_event_record_bytes_are_exactly_sixteen_bytes() {
        let event = TapEvent::new(0x0102_0304, ids::TRANSPORT_FRAME, 0, 0, 0)
            .with_causal_key(0x0506)
            .with_arg0(0x0708_090a)
            .with_arg1(0x0b0c_0d0e);
        assert_eq!(
            event.bytes,
            [
                0x01, 0x02, 0x03, 0x04, 0x02, 0x06, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c,
                0x0d, 0x0e,
            ]
        );
    }

    #[test]
    fn tap_event_debug_uses_semantic_fields_only() {
        let event = TapEvent::new(0x0102_0304, ids::TRANSPORT_FRAME, 0, 0, 0)
            .with_causal_key(0x0506)
            .with_arg0(0x0708_090a)
            .with_arg1(0x0b0c_0d0e);
        let rendered = format!("{event:?}");

        for field in [
            "TapEvent",
            "ts",
            "id",
            "causal_key",
            "arg0",
            "arg1",
            "evidence",
        ] {
            assert!(
                rendered.contains(field),
                "TapEvent Debug must expose semantic field {field}: {rendered}"
            );
        }
        for forbidden in ["bytes", "[u8", "[1, 2, 3, 4", "0x01020304"] {
            assert!(
                !rendered.contains(forbidden),
                "TapEvent Debug must not expose raw record storage {forbidden}: {rendered}"
            );
        }
    }
}
