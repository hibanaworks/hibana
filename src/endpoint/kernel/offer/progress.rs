use crate::endpoint::kernel::frontier::OfferEntryEvidence;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::endpoint::kernel) enum FrontierDeferOutcome {
    Continue,
    Yielded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::endpoint::kernel) struct EvidenceFingerprint(u8);

impl EvidenceFingerprint {
    #[inline]
    pub(in crate::endpoint::kernel) const fn from_offer_entry_evidence(
        evidence: OfferEntryEvidence,
    ) -> Self {
        let mut bits = 0u8;
        if evidence.has_ready_arm() {
            bits |= 1 << 0;
        }
        if evidence.ingress_ready() {
            bits |= 1 << 1;
        }
        Self(bits)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::endpoint::kernel) struct OfferProgressState {
    last_fingerprint: Option<EvidenceFingerprint>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::endpoint::kernel) enum OfferEvidenceOutcome {
    NewEvidence,
    Pending,
}

impl OfferProgressState {
    #[inline]
    pub(in crate::endpoint::kernel) const fn new() -> Self {
        Self {
            last_fingerprint: None,
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn on_defer(
        &mut self,
        fingerprint: EvidenceFingerprint,
    ) -> OfferEvidenceOutcome {
        let has_new_evidence = self.last_fingerprint != Some(fingerprint);
        self.last_fingerprint = Some(fingerprint);
        if has_new_evidence {
            OfferEvidenceOutcome::NewEvidence
        } else {
            OfferEvidenceOutcome::Pending
        }
    }
}

#[cfg(all(test, hibana_repo_tests))]
mod tests;

#[cfg(kani)]
mod kani;
