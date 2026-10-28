use super::{EvidenceFingerprint, OfferEvidenceOutcome, OfferProgressState};

fn evidence(bits: u8) -> EvidenceFingerprint {
    EvidenceFingerprint::new(bits & 1 != 0, bits & 2 != 0)
}

#[kani::proof]
fn offer_progress_classifies_every_evidence_transition_exactly() {
    let first = evidence(kani::any());
    let next = evidence(kani::any());
    let mut progress = OfferProgressState::new();

    assert_eq!(progress.on_defer(first), OfferEvidenceOutcome::NewEvidence);
    assert_eq!(
        progress.on_defer(next),
        if first == next {
            OfferEvidenceOutcome::Pending
        } else {
            OfferEvidenceOutcome::NewEvidence
        }
    );
}
