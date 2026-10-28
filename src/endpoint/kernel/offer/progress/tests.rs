use super::*;

fn evidence(bits: u8) -> EvidenceFingerprint {
    EvidenceFingerprint::new(bits & 1 != 0, bits & 2 != 0)
}

#[test]
fn offer_progress_detects_exact_evidence_changes() {
    for first_bits in 0u8..4 {
        for next_bits in 0u8..4 {
            let first = evidence(first_bits);
            let next = evidence(next_bits);
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
    }
}
