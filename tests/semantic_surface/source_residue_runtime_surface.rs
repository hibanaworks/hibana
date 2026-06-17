use super::common::*;

#[test]
fn production_and_gates_do_not_reintroduce_std_feature_branches() {
    let production = read_production_rs_tree("src");
    let readme = read("README.md");
    let gates = read_tree(".github/scripts");
    let combined = [production.as_str(), readme.as_str(), gates.as_str()].join("\n");
    for forbidden in [
        concat!("cfg(feature = \"", "std", "\")"),
        concat!("cfg(not(feature = \"", "std", "\"))"),
        concat!("features = [\"", "std", "\"]"),
        concat!("--features ", "std"),
        concat!("std ", "feature"),
        concat!("host ", "diagnostics"),
    ] {
        assert!(
            !combined.contains(forbidden),
            "production and gate surface must not reintroduce host cfg branching: {forbidden}"
        );
    }
    assert!(
        read("src/lib.rs").contains("#![no_std]")
            && !read("src/lib.rs").contains("cfg_attr(not(feature"),
        "crate root must be unconditionally no_std"
    );
}

#[test]
fn production_sources_do_not_reintroduce_transport_fragmentation_axis() {
    let production = read_production_rs_tree("src");
    for forbidden in [
        concat!("Frame", "Flags"),
        "flags: Frame",
        concat!("Frame", "Flags::"),
        concat!("Frame", "Flags {"),
    ] {
        assert!(
            !production.contains(forbidden),
            "transport fragmentation vocabulary must not return to production source: {forbidden}"
        );
    }
    for line in production.lines() {
        for forbidden in [concat!("FR", "AG"), concat!("ID", "X"), concat!("TO", "T")] {
            assert!(
                !line
                    .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
                    .any(|token| token == forbidden),
                "transport fragmentation token must not return to production source: {line}"
            );
        }
    }
    for forbidden in [
        "endpoint_resolver_args",
        "emit_endpoint_resolver_audit",
        "ResolverSlot::EndpointRx",
        "ResolverSlot::EndpointTx",
        "hash_tap_event",
        "emit_resolver_audit_replay",
        "EndpointRxAuditPlan",
        "publish_endpoint_rx_audit",
        "build_endpoint_rx_audit_plan",
    ] {
        assert!(
            !production.contains(forbidden),
            "endpoint resolver replay audit vocabulary must not return: {forbidden}"
        );
    }
}

#[test]
fn transport_surface_has_no_custom_error_axis() {
    let transport = read("src/transport.rs");
    let trait_body = transport
        .split("pub trait Transport")
        .nth(1)
        .expect("Transport trait must exist")
        .split("/// Observability helpers")
        .next()
        .expect("Transport trait must precede trace module");
    for forbidden in ["type Error", "Self::Error", "Into<TransportError>"] {
        assert!(
            !trait_body.contains(forbidden),
            "Transport trait must return compact TransportError directly: {forbidden}"
        );
    }

    let transport_boundary = [
        read("src/transport.rs"),
        read("src/endpoint/kernel/lane_port.rs"),
        read("src/rendezvous/port/recv_frame.rs"),
    ]
    .join("\n");
    for forbidden in ["Into<TransportError>", "map_err(Into::into)"] {
        assert!(
            !transport_boundary.contains(forbidden),
            "transport boundary must not keep custom-error erasure residue: {forbidden}"
        );
    }
}

#[test]
fn public_surface_scanner_covers_trait_associated_items() {
    let g_allowlist = read(".github/allowlists/g-public-api.txt");
    let runtime_allowlist = read(".github/allowlists/runtime-public-api.txt");
    let scanner = read(".github/scripts/check_public_api_allowlists.py");

    for required in [
        "Message::LOGICAL_LABEL const LOGICAL_LABEL: u8;",
        "Message::Payload type Payload;",
    ] {
        assert!(
            g_allowlist.contains(required),
            "g public allowlist scanner must cover Message associated item: {required}"
        );
    }
    for required in [
        "Transport::Tx type Tx<'a>: 'a where Self: 'a;",
        "Transport::Rx type Rx<'a>: 'a where Self: 'a;",
        "Transport::open fn open<'a>(&'a self, port: PortOpen) -> (Self::Tx<'a>, Self::Rx<'a>);",
        "Transport::poll_send fn poll_send<'a, 'f>( &self, tx: &'a mut Self::Tx<'a>, outgoing: Outgoing<'f>, cx: &mut Context<'_>, ) -> Poll<Result<(), TransportError>> where 'a: 'f;",
        "Transport::cancel_send fn cancel_send<'a>(&self, tx: &'a mut Self::Tx<'a>);",
        "Transport::poll_recv fn poll_recv<'a>( &'a self, rx: &'a mut Self::Rx<'a>, cx: &mut Context<'_>, ) -> Poll<Result<ReceivedFrame<'a>, TransportError>>;",
        "Transport::requeue fn requeue<'a>(&self, rx: &mut Self::Rx<'a>) -> Result<(), TransportError>;",
        "WireEncode::encode_into fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError>;",
        "WirePayload::Decoded type Decoded<'a>;",
        "WirePayload::validate_payload fn validate_payload(input: Payload<'_>) -> Result<(), CodecError>;",
        "WirePayload::decode_validated_payload fn decode_validated_payload<'a>(input: Payload<'a>) -> Self::Decoded<'a>;",
        "WirePayload::decode_payload fn decode_payload<'a>(input: Payload<'a>) -> Result<Self::Decoded<'a>, CodecError> {",
    ] {
        assert!(
            runtime_allowlist.contains(required),
            "runtime public allowlist scanner must cover trait associated item: {required}"
        );
    }
    for forbidden in [
        "Message::Decoded",
        "WireEncode::encoded_len",
        "Transport::poll_flush",
        "WirePayload::zero_payload",
    ] {
        assert!(
            !g_allowlist.contains(forbidden) && !runtime_allowlist.contains(forbidden),
            "public allowlists must fail closed for removed trait item: {forbidden}"
        );
    }
    assert!(
        scanner.contains("trait_owner_at")
            && scanner.contains("is_trait_item_start")
            && scanner.contains("trait_item_name")
            && scanner.contains("src/global/message.rs")
            && scanner.contains("src/transport/wire.rs"),
        "stable source scanner must include public trait associated items and their owner files"
    );
}

#[test]
fn tap_reader_surface_stays_minimal() {
    let event = read("src/observe/event.rs");
    let allowlist = read(".github/allowlists/runtime-public-api.txt");
    let tap_event_attrs = event
        .split("pub struct TapEvent")
        .next()
        .expect("TapEvent declaration must exist")
        .rsplit("#[derive")
        .next()
        .expect("TapEvent derive attributes must be visible");
    assert!(
        !tap_event_attrs.contains("Debug"),
        "TapEvent must not derive raw storage Debug"
    );
    assert!(
        event.contains("impl core::fmt::Debug for TapEvent"),
        "TapEvent Debug must stay semantic instead of exposing raw bytes"
    );
    for required in [
        "TapEvent::ts",
        "TapEvent::id",
        "TapEvent::causal_key",
        "TapEvent::arg0",
        "TapEvent::arg1",
        "TapEvent::evidence",
        "Evidence::kind",
        "Evidence::reason",
        "Evidence::input",
    ] {
        assert!(
            allowlist.contains(required),
            "runtime allowlist must include canonical tap reader: {required}"
        );
    }
    for forbidden in [
        "pub const fn causal_role",
        "pub const fn causal_seq",
        "pub const fn input_word",
        "TapEvent::causal_role",
        "TapEvent::causal_seq",
        "Evidence::input_word",
    ] {
        assert!(
            !event.contains(forbidden) && !allowlist.contains(forbidden),
            "tap derived convenience helper must not be public: {forbidden}"
        );
    }
}
