use super::SessionFaultKind;

#[kani::proof]
fn session_fault_encoding_roundtrip_is_exact() {
    let fault: SessionFaultKind = kani::any();
    let encoded = fault.encode();

    assert!(encoded != SessionFaultKind::ABSENT_CODE);
    assert!(SessionFaultKind::decode(encoded) == Some(fault));
    kani::cover!(fault == SessionFaultKind::TransportClosed);
    kani::cover!(fault == SessionFaultKind::DecodeFailed);
    kani::cover!(fault == SessionFaultKind::ProtocolViolation);
    kani::cover!(fault == SessionFaultKind::EndpointDropped);
    kani::cover!(fault == SessionFaultKind::ProgressInvariantViolated);
}

#[kani::proof]
fn session_fault_encoding_is_injective() {
    let left: SessionFaultKind = kani::any();
    let right: SessionFaultKind = kani::any();

    assert!((left.encode() == right.encode()) == (left == right));
}

#[kani::proof]
fn session_fault_checked_decoding_domain_is_exact() {
    let raw: u8 = kani::any();
    let checked = SessionFaultKind::try_decode(raw);

    assert_eq!(checked.is_some(), raw <= 5);
    match (raw, checked) {
        (0, Some(None)) => {}
        (1, Some(Some(SessionFaultKind::TransportClosed))) => {}
        (2, Some(Some(SessionFaultKind::DecodeFailed))) => {}
        (3, Some(Some(SessionFaultKind::ProtocolViolation))) => {}
        (4, Some(Some(SessionFaultKind::EndpointDropped))) => {}
        (5, Some(Some(SessionFaultKind::ProgressInvariantViolated))) => {}
        (6..=u8::MAX, None) => {}
        _ => assert!(false),
    }
}

#[kani::proof]
#[kani::should_panic]
fn session_fault_decoder_rejects_first_invalid_code() {
    let _ = SessionFaultKind::decode(6);
}
