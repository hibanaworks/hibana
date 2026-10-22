/// Choreography-facing message / branch identity.
///
/// This is intentionally crate-private. Application code expresses logical
/// labels through `g::Msg<L, P>` and observes them through `RouteBranch`.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct LogicalLabel(u8);

impl LogicalLabel {
    #[inline]
    pub(crate) const fn new(raw: u8) -> Self {
        Self(raw)
    }

    #[inline]
    pub(crate) const fn raw(self) -> u8 {
        self.0
    }
}

/// Position of a message in the shared global protocol.
///
/// Distinct message sites have distinct values, including ordered messages
/// with the same logical `g::Msg` label. Repetition reuses the same site;
/// transports must still exclude replay and isolate session runs.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FrameLabel(u16);

impl FrameLabel {
    #[inline]
    pub(crate) const fn new(raw: u16) -> Self {
        Self(raw)
    }

    /// Return this message's position in the shared global protocol.
    #[inline]
    pub const fn raw(self) -> u16 {
        self.0
    }
}
