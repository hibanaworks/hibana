#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SessionFaultKind {
    TransportClosed,
    PeerReset,
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
            Self::PeerReset => 2,
            Self::DecodeFailed => 3,
            Self::ProtocolViolation => 4,
            Self::EndpointDropped => 5,
            Self::ProgressInvariantViolated => 6,
        }
    }

    pub(super) const fn decode(raw: u8) -> Option<Self> {
        match raw {
            Self::ABSENT_CODE => None,
            1 => Some(Self::TransportClosed),
            2 => Some(Self::PeerReset),
            3 => Some(Self::DecodeFailed),
            4 => Some(Self::ProtocolViolation),
            5 => Some(Self::EndpointDropped),
            6 => Some(Self::ProgressInvariantViolated),
            _ => crate::invariant(),
        }
    }
}
