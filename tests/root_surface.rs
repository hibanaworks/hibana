use std::fs;
use std::path::PathBuf;

fn lib_rs() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs");
    fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("read {} failed: {}", path.display(), err))
}

fn endpoint_rs() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/endpoint.rs");
    fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("read {} failed: {}", path.display(), err))
}

fn global_rs() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/global.rs");
    fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("read {} failed: {}", path.display(), err))
}

fn g_rs() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/g.rs");
    fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("read {} failed: {}", path.display(), err))
}

fn public_api_script_rs() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".github/scripts/check_hibana_public_api.sh");
    fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("read {} failed: {}", path.display(), err))
}

fn compact_ws(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut prev_space = false;
    for ch in input.chars() {
        if ch.is_whitespace() {
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
        } else {
            out.push(ch);
            prev_space = false;
        }
    }
    out
}

#[test]
fn root_visible_surface_stays_minimal() {
    let lib_rs = lib_rs();
    let endpoint_rs = endpoint_rs();
    let global_rs = global_rs();
    let g_rs = g_rs();
    let g_ws = compact_ws(&g_rs);
    let advanced_head = {
        let start = global_rs
            .find("pub mod advanced {")
            .expect("advanced surface block must exist");
        let rest = &global_rs[start..];
        let end = rest
            .find("    pub mod steps {")
            .expect("advanced steps block must exist");
        &rest[..end]
    };

    assert!(
        lib_rs.contains("pub mod g;"),
        "hibana root must expose g surface"
    );
    assert!(
        g_ws.contains("pub use crate::global::advanced;")
            && (g_ws.contains("pub use crate::global::program::Program;")
                || g_ws.contains(
                    "pub use crate::global::{Msg, Program, Role, par, route, send, seq};"
                ))
            && g_ws.contains("pub use crate::global::{Msg, Role, par, route, send, seq};"),
        "hibana::g root must stay on the canonical app primitives"
    );
    assert!(
        lib_rs.contains("pub mod substrate;"),
        "hibana root must expose substrate surface"
    );
    assert!(
        lib_rs.contains("pub use endpoint::{Endpoint, RecvError, RecvResult, RouteBranch, SendError, SendResult};"),
        "hibana root must expose endpoint core API"
    );

    for forbidden in [
        "pub use global::{",
        "pub use binding::{",
        "pub use control::cap::{",
        "pub use control::types::LaneId as Lane;",
        "pub use control::types::{Gen, LaneId, RendezvousId, SessionId};",
        "pub use endpoint::{ControlOutcome, CursorEndpoint};",
        "pub use epf::Slot;",
        "pub use epf::TapEvent;",
        "pub use runtime::SessionCluster;",
        "pub use runtime::config::{Clock, CounterClock};",
        "pub use runtime::consts::{DEFAULT_LABEL_UNIVERSE, LabelUniverse};",
        "pub mod global;",
        "pub mod control;",
        "pub mod runtime;",
        "pub mod transport;",
        "pub mod observe;",
        "pub use crate::global::{par_chain, route_chain};",
    ] {
        assert!(
            !lib_rs.contains(forbidden),
            "hibana root must not re-export legacy/internal substrate names: {forbidden}"
        );
    }

    for forbidden in ["par_chain", "route_chain"] {
        assert!(
            !g_rs.contains(forbidden),
            "hibana::g root must not expose hidden builder aliases: {forbidden}"
        );
    }

    for forbidden in [
        "pub fn policy(",
        "pub const fn policy(",
        " pub use crate::global::{Msg, Program, Role, par, policy, route, send, seq};",
    ] {
        assert!(
            !g_rs.contains(forbidden),
            "hibana::g root must not expose a top-level policy helper: {forbidden}"
        );
    }

    for forbidden in [
        "pub use cursor::{CursorEndpoint, RouteBranch};",
        "pub use cursor::CursorEndpoint;",
    ] {
        assert!(
            !endpoint_rs.contains(forbidden),
            "endpoint module must not re-export CursorEndpoint on the app-facing path: {forbidden}"
        );
    }

    for forbidden in [
        "CanonicalControl, ControlHandling, ControlMessage,",
        "ExternalControl,\n        LoopBreakSteps,",
        "PolicyMode,",
        "const_dsl::{\n            ControlScopeKind, DynamicMeta,",
        "LocalProgram,",
        "NoControl,",
        "project_chain",
        "project, project_ref,",
        "project,\n        with_policy,",
        "typestate::{JumpReason, LocalAction, PassiveArmNavigation, PhaseCursor}",
    ] {
        assert!(
            !advanced_head.contains(forbidden),
            "g::advanced root must not re-export typestate/internal helper: {forbidden}"
        );
    }

    for required in [
        "RoleProgram",
        "project",
        "const_dsl::EffList",
        "CanonicalControl",
    ] {
        assert!(
            advanced_head.contains(required),
            "g::advanced root must stay on projection + control-message SPI only: {required}"
        );
    }

    for forbidden in [
        "pub mod control {",
        "pub mod metadata {",
        "pub mod loops {",
        "pub mod typestate {",
        "project_ref",
        "route_chain",
        "par_chain",
    ] {
        assert!(
            !global_rs.contains(forbidden),
            "deleted lower-layer helper must not remain in global surface source: {forbidden}"
        );
    }
}

#[test]
fn public_api_gate_tracks_g_and_substrate_surfaces() {
    let script = public_api_script_rs();

    for required in [
        "target/doc/hibana.json",
        "cargo +nightly rustdoc --lib --features std -- -Z unstable-options --output-format json",
        "HIBANA_RUSTDOC_JSON",
        "cargo +nightly test --test semantic_surface --features std",
        "semantic public API check passed",
    ] {
        assert!(
            script.contains(required),
            "crate-local public API gate must run the nightly rustdoc semantic verifier: {required}"
        );
    }
}
