use super::OfferCursorReadiness;

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel::offer) enum OfferArmRecvEvidence {
    HasRecv,
    Recvless,
}

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel::offer) enum OfferControllerCursorArm {
    AtArm,
    OutsideArm,
}

impl OfferControllerCursorArm {
    #[inline]
    const fn is_outside_arm(self) -> bool {
        matches!(self, Self::OutsideArm)
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
pub(in crate::endpoint::kernel::offer) struct OfferControllerLocalEvidence {
    cursor: OfferCursorReadiness,
    cursor_arm: OfferControllerCursorArm,
    materialization: OfferMaterializationReadiness,
}

impl OfferControllerLocalEvidence {
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
        matches!(self.cursor, OfferCursorReadiness::NonRecv) && self.cursor_arm.is_outside_arm()
    }
}

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel::offer) enum OfferPassiveReadySignal {
    Observed,
    Absent,
}

impl OfferPassiveReadySignal {
    #[inline]
    const fn is_observed(self) -> bool {
        matches!(self, Self::Observed)
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
pub(in crate::endpoint::kernel::offer) struct OfferPassiveEvidence {
    ready_signal: OfferPassiveReadySignal,
    recv: OfferPassiveRecvEvidence,
}

impl OfferPassiveEvidence {
    #[inline]
    pub(in crate::endpoint::kernel::offer) const fn new(
        ready_signal: OfferPassiveReadySignal,
        recv: OfferPassiveRecvEvidence,
    ) -> Self {
        Self { ready_signal, recv }
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer) const fn has_ready_signal(self) -> bool {
        self.ready_signal.is_observed()
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer) const fn dynamic_scope_without_recv(self) -> bool {
        self.recv.is_recvless()
    }
}
