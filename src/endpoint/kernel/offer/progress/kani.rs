use super::{EvidenceFingerprint, OfferEvidenceOutcome, OfferProgressState};
use crate::endpoint::kernel::frontier::OfferEntryEvidence;

fn evidence(bits: u8) -> OfferEntryEvidence {
    let evidence = OfferEntryEvidence::empty();
    let evidence = if bits & 1 != 0 {
        evidence.with_ready_arm()
    } else {
        evidence
    };
    if bits & 2 != 0 {
        evidence.with_ingress_ready()
    } else {
        evidence
    }
}

#[kani::proof]
fn offer_progress_classifies_every_evidence_transition_exactly() {
    let first = evidence(kani::any());
    let next = evidence(kani::any());
    let mut progress = OfferProgressState::new();

    assert_eq!(
        progress.on_defer(EvidenceFingerprint::from_offer_entry_evidence(first)),
        OfferEvidenceOutcome::NewEvidence
    );
    assert_eq!(
        progress.on_defer(EvidenceFingerprint::from_offer_entry_evidence(next)),
        if first == next {
            OfferEvidenceOutcome::Pending
        } else {
            OfferEvidenceOutcome::NewEvidence
        }
    );
}
