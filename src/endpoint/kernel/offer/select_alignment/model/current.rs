use super::super::super::CurrentFrontierSelectionState;
use super::entry::ProgressEvidence;
use crate::endpoint::kernel::frontier::ExactOfferObservation;

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel::offer::select_alignment) struct CurrentOfferObservation {
    flags: u8,
}

impl CurrentOfferObservation {
    const PRESENT: u8 = 1;
    const SELECTABLE: u8 = 1 << 1;
    const READY: u8 = 1 << 2;
    const PROGRESS_EVIDENCE: u8 = 1 << 3;
    const OBSERVED_PROGRESS_EVIDENCE: u8 = 1 << 4;
    const READY_ARM_EVIDENCE: u8 = 1 << 5;
    const CONTROLLER_PROGRESS_SIBLING_EXISTS: u8 = 1 << 6;

    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn empty() -> Self {
        Self { flags: 0 }
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn from_exact(
        exact: Option<ExactOfferObservation>,
    ) -> Self {
        let Some(exact) = exact else {
            return Self::empty();
        };
        let mut flags = Self::PRESENT;
        if exact.is_selectable() {
            flags |= Self::SELECTABLE;
            if exact.is_ready() {
                flags |= Self::READY;
            }
            if exact.has_progress() {
                flags |= Self::PROGRESS_EVIDENCE;
            }
        }
        if exact.has_progress() {
            flags |= Self::OBSERVED_PROGRESS_EVIDENCE;
        }
        if exact.has_ready_arm() {
            flags |= Self::READY_ARM_EVIDENCE;
        }
        Self { flags }
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn with_controller_progress_sibling(
        self,
    ) -> Self {
        Self {
            flags: self.flags | Self::CONTROLLER_PROGRESS_SIBLING_EXISTS,
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn present(self) -> bool {
        (self.flags & Self::PRESENT) != 0
    }

    /// An absent exact row leaves the descriptor-owned current position
    /// unchanged. Once an exact row exists, only admitted evidence may retain
    /// that position.
    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn permits_retention(
        self,
    ) -> bool {
        !self.present() || self.selectable()
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn selectable(self) -> bool {
        (self.flags & Self::SELECTABLE) != 0
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn ready(self) -> bool {
        (self.flags & Self::READY) != 0
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn progress_evidence(
        self,
    ) -> bool {
        (self.flags & Self::PROGRESS_EVIDENCE) != 0
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn has_authority_evidence(
        self,
    ) -> bool {
        (self.flags & (Self::READY_ARM_EVIDENCE | Self::OBSERVED_PROGRESS_EVIDENCE)) != 0
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn has_ready_arm_evidence(
        self,
    ) -> bool {
        (self.flags & Self::READY_ARM_EVIDENCE) != 0
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn accumulated_progress_evidence(
        self,
        state: CurrentFrontierSelectionState,
    ) -> ProgressEvidence {
        if (self.flags & Self::OBSERVED_PROGRESS_EVIDENCE) != 0 || state.has_progress_evidence() {
            ProgressEvidence::Present
        } else {
            ProgressEvidence::Absent
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn controller_progress_sibling_exists(
        self,
    ) -> bool {
        (self.flags & Self::CONTROLLER_PROGRESS_SIBLING_EXISTS) != 0
    }
}
