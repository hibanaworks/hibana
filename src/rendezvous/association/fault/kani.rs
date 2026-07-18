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
#[kani::should_panic]
fn invalid_session_fault_encoding_is_fail_fast() {
    let raw = (6u16 + u16::from(kani::any::<u8>()) % 250) as u8;
    let _ = SessionFaultKind::decode(raw);
}
