#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(kani, derive(kani::Arbitrary))]
pub(crate) enum SessionFaultKind {
    TransportClosed,
    DecodeFailed,
    ProtocolViolation,
    EndpointDropped,
    ProgressInvariantViolated,
}

impl SessionFaultKind {
    pub(super) const ABSENT_CODE: u8 = 0;

    pub(super) const fn encode(self) -> u8 {
        match self {
            Self::TransportClosed => 1,
            Self::DecodeFailed => 2,
            Self::ProtocolViolation => 3,
            Self::EndpointDropped => 4,
            Self::ProgressInvariantViolated => 5,
        }
    }

    /// The outer option rejects malformed bytes; the inner option represents
    /// the canonical no-fault code.
    pub(super) const fn try_decode(raw: u8) -> Option<Option<Self>> {
        match raw {
            Self::ABSENT_CODE => Some(None),
            1 => Some(Some(Self::TransportClosed)),
            2 => Some(Some(Self::DecodeFailed)),
            3 => Some(Some(Self::ProtocolViolation)),
            4 => Some(Some(Self::EndpointDropped)),
            5 => Some(Some(Self::ProgressInvariantViolated)),
            _ => None,
        }
    }

    pub(super) const fn decode(raw: u8) -> Option<Self> {
        match Self::try_decode(raw) {
            Some(fault) => fault,
            None => crate::invariant(),
        }
    }
}

#[cfg(kani)]
#[path = "fault/kani.rs"]
mod kani_proofs;
