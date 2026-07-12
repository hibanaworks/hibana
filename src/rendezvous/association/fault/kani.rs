use super::SessionFaultKind;

const SYMBOLIC_FAULTS: [SessionFaultKind; 5] = [
    SessionFaultKind::TransportClosed,
    SessionFaultKind::DecodeFailed,
    SessionFaultKind::ProtocolViolation,
    SessionFaultKind::EndpointDropped,
    SessionFaultKind::ProgressInvariantViolated,
];

fn symbolic_fault(raw: u8) -> SessionFaultKind {
    SYMBOLIC_FAULTS[usize::from(raw) % SYMBOLIC_FAULTS.len()]
}

#[kani::proof]
fn session_fault_encoding_roundtrip_is_exact() {
    let raw: u8 = kani::any();
    let fault = symbolic_fault(raw);
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
    let left = symbolic_fault(kani::any());
    let right = symbolic_fault(kani::any());

    assert!((left.encode() == right.encode()) == (left == right));
}

#[kani::proof]
#[kani::should_panic]
fn invalid_session_fault_encoding_is_fail_fast() {
    let raw = (6u16 + u16::from(kani::any::<u8>()) % 250) as u8;
    let _ = SessionFaultKind::decode(raw);
}
