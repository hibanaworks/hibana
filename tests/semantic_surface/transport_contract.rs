use super::common::*;

#[test]
fn transport_contract_separates_local_protocol_safety_from_affine_delivery() {
    let transport = read("src/transport.rs");
    let contract_tests = read("src/transport/tests.rs");
    let integration_transport = read("tests/common/mod.rs");

    for required in [
        "Every successful [`poll_recv`](Transport::poll_recv) yields one carrier",
        "does not prove delivery",
        "Global fidelity and progress additionally assume affine delivery",
        "cannot authenticate carrier identity",
        "bound to the mapped peer/direction",
        "make loss, retry, and freshness",
        "This trait neither receives nor negotiates compiled protocol images.",
        "Address migration or identifier rotation within the",
        "Replayable early data may be exposed only after",
        "The strong affine-delivery profile also requires observable peer closure",
        "no global cancellation-termination",
        "cancellation must retire that logical direction",
        "fn abort_peer(&mut self)",
        "transport_contract_abort_retires_undelivered_tail_and_closes",
        "transport_contract_carrier_generation_isolates_session_reuse",
    ] {
        assert!(
            transport.contains(required) || contract_tests.contains(required),
            "transport contract must preserve protocol-neutral endpoint semantics: {required}"
        );
    }
    for forbidden in [
        "CompiledProgramRef",
        "protocol_image",
        "bind_protocol_image_pair",
        "Returning `Pending` forever after peer closure violates this trait.",
    ] {
        assert!(
            !transport.contains(forbidden) && !contract_tests.contains(forbidden),
            "transport must not regain descriptor/image handshake ownership: {forbidden}"
        );
    }
    assert!(
        integration_transport.contains("frame.session_id == session_id && frame.lane == lane")
            && integration_transport.contains("state.dequeue(rx.role, rx.session_id, rx.lane)"),
        "multi-session transport tests must demultiplex by session and lane"
    );
}
