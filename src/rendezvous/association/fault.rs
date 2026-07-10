#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

    pub(super) const fn decode(raw: u8) -> Option<Self> {
        match raw {
            Self::ABSENT_CODE => None,
            1 => Some(Self::TransportClosed),
            2 => Some(Self::DecodeFailed),
            3 => Some(Self::ProtocolViolation),
            4 => Some(Self::EndpointDropped),
            5 => Some(Self::ProgressInvariantViolated),
            _ => crate::invariant(),
        }
    }
}
