use super::*;

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

#[test]
fn offer_progress_detects_exact_evidence_changes() {
    for first_bits in 0u8..4 {
        for next_bits in 0u8..4 {
            let first = evidence(first_bits);
            let next = evidence(next_bits);
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
    }
}
