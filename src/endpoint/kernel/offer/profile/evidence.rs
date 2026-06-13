use super::{OfferCursorReadiness, OfferEarlyDecisionReadiness};

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel::offer) enum OfferArmRecvEvidence {
    HasRecv,
    Recvless,
}

impl OfferEarlyDecisionReadiness {
    #[inline]
    pub(in crate::endpoint::kernel::offer) const fn from_arm_evidence(
        evidence: Option<OfferArmRecvEvidence>,
    ) -> Self {
        match evidence {
            None => Self::Unavailable,
            Some(OfferArmRecvEvidence::Recvless) => Self::AvailableWithoutRecv,
            Some(OfferArmRecvEvidence::HasRecv) => Self::AvailableWithRecv,
        }
    }
}

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel::offer) enum OfferControllerCursorArm {
    Present,
    Missing,
}

impl OfferControllerCursorArm {
    #[inline]
    const fn is_missing(self) -> bool {
        matches!(self, Self::Missing)
    }
}

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel::offer) enum OfferMaterializationReadiness {
    Ready,
    Pending,
}

impl OfferMaterializationReadiness {
    #[inline]
    const fn is_pending(self) -> bool {
        matches!(self, Self::Pending)
    }
}

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel::offer) struct OfferControllerSkipEvidence {
    cursor: OfferCursorReadiness,
    cursor_arm: OfferControllerCursorArm,
    materialization: OfferMaterializationReadiness,
}

impl OfferControllerSkipEvidence {
    #[inline]
    pub(in crate::endpoint::kernel::offer) const fn new(
        cursor: OfferCursorReadiness,
        cursor_arm: OfferControllerCursorArm,
        materialization: OfferMaterializationReadiness,
    ) -> Self {
        Self {
            cursor,
            cursor_arm,
            materialization,
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer) const fn materialization_pending(self) -> bool {
        self.materialization.is_pending()
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer) const fn non_entry_cursor_ready(self) -> bool {
        matches!(self.cursor, OfferCursorReadiness::NonRecv) && self.cursor_arm.is_missing()
    }
}

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel::offer) enum OfferPassiveReadySignal {
    Present,
    Missing,
}

impl OfferPassiveReadySignal {
    #[inline]
    const fn is_present(self) -> bool {
        matches!(self, Self::Present)
    }
}

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel::offer) enum OfferPassiveRecvEvidence {
    HasRecv,
    Recvless,
}

impl OfferPassiveRecvEvidence {
    #[inline]
    const fn is_recvless(self) -> bool {
        matches!(self, Self::Recvless)
    }
}

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel::offer) enum OfferPassiveAckEvidence {
    Materializable,
    NotMaterializable,
}

impl OfferPassiveAckEvidence {
    #[inline]
    const fn is_materializable(self) -> bool {
        matches!(self, Self::Materializable)
    }
}

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel::offer) struct OfferPassiveEvidence {
    ready_signal: OfferPassiveReadySignal,
    recv: OfferPassiveRecvEvidence,
    ack: OfferPassiveAckEvidence,
}

impl OfferPassiveEvidence {
    #[inline]
    pub(in crate::endpoint::kernel::offer) const fn new(
        ready_signal: OfferPassiveReadySignal,
        recv: OfferPassiveRecvEvidence,
        ack: OfferPassiveAckEvidence,
    ) -> Self {
        Self {
            ready_signal,
            recv,
            ack,
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer) const fn has_ready_signal(self) -> bool {
        self.ready_signal.is_present()
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer) const fn dynamic_scope_without_recv(self) -> bool {
        self.recv.is_recvless()
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer) const fn ack_materializable(self) -> bool {
        self.ack.is_materializable()
    }
}
