use super::common::*;

fn call_args(source: &str, name: &str) -> Vec<String> {
    let pattern = format!("{name}(");
    let mut out = Vec::new();
    let mut cursor = 0;
    while let Some(found) = source[cursor..].find(&pattern) {
        let args_start = cursor + found + pattern.len();
        let mut depth = 0usize;
        let mut args_end = None;
        for (offset, ch) in source[args_start..].char_indices() {
            match ch {
                '(' => depth += 1,
                ')' if depth == 0 => {
                    args_end = Some(args_start + offset);
                    break;
                }
                ')' => depth -= 1,
                _ => {}
            }
        }
        if let Some(end) = args_end {
            out.push(source[args_start..end].to_owned());
            cursor = end + 1;
        } else {
            break;
        }
    }
    out
}

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
    for args in call_args(&production, "endpoint_resolver_args") {
        let arg_count = args.split(',').filter(|arg| !arg.trim().is_empty()).count();
        assert!(
            arg_count <= 2,
            "endpoint resolver audit args must not accept a flags axis: {args}"
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
