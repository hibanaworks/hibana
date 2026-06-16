use std::fs;
use std::path::PathBuf;

fn read(path: &str) -> String {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let full = root.join(path);
    fs::read_to_string(&full)
        .unwrap_or_else(|err| panic!("read {} failed: {}", full.display(), err))
}

fn read_dir_rs(path: &str) -> String {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path);
    let mut parts = fs::read_dir(&root)
        .unwrap_or_else(|err| panic!("read {} failed: {}", root.display(), err))
        .map(|entry| {
            entry
                .unwrap_or_else(|err| {
                    panic!("read dir entry in {} failed: {}", root.display(), err)
                })
                .path()
        })
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("rs"))
        .collect::<Vec<_>>();
    parts.sort();
    parts
        .into_iter()
        .map(|part| {
            fs::read_to_string(&part)
                .unwrap_or_else(|err| panic!("read {} failed: {}", part.display(), err))
        })
        .collect::<Vec<_>>()
        .join("")
}

fn runtime_source() -> String {
    let mut source = read("src/runtime.rs");
    source.push_str(&read_dir_rs("src/runtime"));
    source
}

fn transport_source() -> String {
    let mut source = read("src/transport.rs");
    source.push_str(&read_dir_rs("src/transport"));
    source
}

#[test]
fn transport_resolver_signal_surface_stays_minimal() {
    let transport_src = transport_source();
    let runtime_src = runtime_source();
    let readme_src = read("README.md");

    for (source, forbidden, why) in [
        (
            transport_src.as_str(),
            "pub struct TransportSnapshot",
            "TransportSnapshot must stay internal",
        ),
        (
            transport_src.as_str(),
            "TransportSnapshotParts",
            "transport snapshot option-bag constructor must not exist",
        ),
        (
            transport_src.as_str(),
            "from_parts(parts:",
            "transport snapshot parts constructor must not exist",
        ),
        (
            runtime_src.as_str(),
            "TransportSnapshotParts",
            "runtime::transport must not re-export TransportSnapshotParts",
        ),
        (
            runtime_src.as_str(),
            "TransportSnapshot",
            "runtime::transport must not re-export TransportSnapshot",
        ),
        (
            readme_src.as_str(),
            "TransportSnapshot",
            "README must not publish the forbidden TransportSnapshot surface",
        ),
    ] {
        assert!(!source.contains(forbidden), "{why}: {forbidden}");
    }
    assert!(
        !transport_src.contains("fn resolver_attrs(&self)")
            && !transport_src.contains("pub trait TransportMetrics")
            && !transport_src.contains("type Metrics"),
        "Transport must not expose resolver input, metrics, or telemetry extra hooks"
    );
    assert!(
        !readme_src.contains("ResolverAttrs") && readme_src.contains("ResolverRef::decision_state"),
        "README must keep replay attrs out of the canonical path and describe resolver-state owned input"
    );
    for forbidden in [
        "ContextId",
        "ContextValue",
        "ResolverInput",
        "ResolverSignals",
        "ResolverAttrs",
        "pub mod core {",
        "pub mod replay {",
        "advanced::resolver",
    ] {
        assert!(
            !runtime_src.contains(forbidden),
            "resolver signal extension namespace must not leak through runtime: {forbidden}"
        );
    }
    for forbidden in [
        "pub const fn new(latency_us: Option<u64>, queue_depth: Option<u32>) -> Self",
        "pub const fn with_latency_us",
        "pub const fn with_queue_depth",
        "pub const fn with_congestion_marks",
        "pub const fn with_retransmissions",
        "pub const fn with_congestion_window",
        "pub const fn with_in_flight",
        "pub const fn with_algorithm",
    ] {
        assert!(
            !transport_src.contains(forbidden),
            "transport snapshot builder surface must stay forbidden: {forbidden}"
        );
    }
    for forbidden in [
        "TransportMetricsTapPayload,",
        "pub primary: (u32, u32)",
        "pub extension: Option<(u32, u32)>",
        "pub kind: TransportEventKind",
        "pub packet_number: u64",
        "pub payload_len: u32",
        "pub retransmissions: u32",
        "pub pn_space: u8",
        "pub cid_tag: u8",
        "pub struct TransportEventMeta",
        "pub packet_number: u64",
        "pub retransmissions: u32",
        "pub const fn new(\n        kind: TransportEventKind,\n        packet_number: u64",
        "pub const fn packet_number(",
        "pub const fn payload_len(",
        "pub const fn retry_count(",
        "pub const fn domain(",
        "pub const fn carrier_tag(",
        "pub const fn retransmissions(",
        "pub const fn packet_number_space(",
        "pub const fn connection_id_tag(",
        "pub const fn new_with_metadata",
        "pub const fn with_pn_space",
        "pub const fn with_cid_tag",
    ] {
        assert!(
            !transport_src.contains(forbidden) && !runtime_src.contains(forbidden),
            "transport observation detail must stay accessor-only and non-literal: {forbidden}"
        );
    }
    assert!(
        !transport_src.contains("TransportEventKind")
            && !transport_src.contains("pub struct TransportEvent")
            && !runtime_src.contains("TransportEventKind")
            && !runtime_src.contains("TransportEvent"),
        "transport telemetry vocabulary must not be part of the protocol-neutral public surface"
    );
}
