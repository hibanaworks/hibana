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

fn substrate_rs() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/substrate.rs");
    fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("read {} failed: {}", path.display(), err))
}

fn g_rs() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/g.rs");
    fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("read {} failed: {}", path.display(), err))
}

fn flow_rs() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/endpoint/flow.rs");
    fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("read {} failed: {}", path.display(), err))
}

fn role_program_rs() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/global/role_program.rs");
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
    let substrate_rs = substrate_rs();
    let g_rs = g_rs();
    let flow_rs = flow_rs();
    let role_program_rs = role_program_rs();
    let g_ws = compact_ws(&g_rs);
    let program_head = {
        let start = substrate_rs
            .find("pub mod program {")
            .expect("substrate program surface block must exist");
        let rest = &substrate_rs[start..];
        let end = rest
            .find("/// Everyday runtime setup owners")
            .expect("program surface block must end before runtime surface");
        &rest[..end]
    };

    assert!(
        lib_rs.contains("pub mod g;"),
        "hibana root must expose g surface"
    );
    assert!(
        g_ws.contains("pub use crate::global::program::Program;")
            && g_ws.contains("pub use crate::global::{Msg, Role, par, route, send, seq};"),
        "hibana::g root must stay on the canonical app primitives"
    );
    assert!(
        !g_ws.contains("advanced") && !global_rs.contains("pub mod advanced {"),
        "hibana::g must not expose protocol-implementor SPI"
    );
    assert!(
        lib_rs.contains("pub mod substrate;"),
        "hibana root must expose substrate surface"
    );
    assert!(
        lib_rs.contains("pub use endpoint::{Endpoint, RecvError, RecvResult, RouteBranch, SendError, SendResult};"),
        "hibana root must expose endpoint core API"
    );

    for (owner, source) in [
        ("lib.rs", lib_rs.as_str()),
        ("global.rs", global_rs.as_str()),
        ("substrate.rs", substrate_rs.as_str()),
        ("role_program.rs", role_program_rs.as_str()),
        ("endpoint/flow.rs", flow_rs.as_str()),
    ] {
        assert!(
            !source.contains("#[allow(private_bounds)]"),
            "{owner} must use an explicit expect or sealed API shape, not a blanket private_bounds allow"
        );
    }

    for forbidden in [
        "pub use global::{",
        "pub use binding::{",
        "pub use control::cap::{",
        "pub use control::types::LaneId as Lane;",
        "pub use control::types::{Gen, LaneId, RendezvousId, SessionId};",
        "pub use endpoint::{ControlEmission, CursorEndpoint};",
        "pub use endpoint::{ControlOutcome, CursorEndpoint};",
        "pub use epf::Slot;",
        "pub use epf::TapEvent;",
        "pub use runtime::SessionKit;",
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
            "hibana root must not re-export non-canonical/internal substrate names: {forbidden}"
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
        "AllowsEndpointMint",
        "MintConfigMarker",
        "MintConfig",
        "#[allow(private_bounds)]",
        concat!("Flow", "Send", "Arg"),
        concat!("Send", "Outcome", "Kind"),
        concat!("pub struct ", "Send", "Value"),
        "trait SendArg",
        "trait SendOutcome",
        "HibanaSend",
        concat!("Cap", "Flow"),
        concat!("Flow", "Inner"),
        "CapRegisteredToken",
        concat!("Cap", "Flow", "Token"),
        "CapFrameToken",
    ] {
        assert!(
            !flow_rs.contains(forbidden),
            "app-facing flow surface must not mention mint-policy internals: {forbidden}"
        );
    }

    for forbidden in [
        "CapRegisteredToken",
        concat!("Cap", "Flow", "Token"),
        "CapFrameToken",
        "pub(crate) mod handle;",
    ] {
        assert!(
            !lib_rs.contains(forbidden)
                && !global_rs.contains(forbidden)
                && !substrate_rs.contains(forbidden)
                && !g_rs.contains(forbidden),
            "removed capability shim and cfg-test owners must not re-enter the public crate surface: {forbidden}"
        );
    }

    for forbidden in [
        concat!("Canonical", "Control"),
        concat!("External", "Control"),
        concat!("Control", "Handling"),
        concat!("Control", "Message", "Kind"),
        concat!("Control", "Message"),
        concat!("Loop", "Break", "Steps,"),
        "PolicyMode,",
        "const_dsl::{\n            ControlScopeKind, DynamicMeta,",
        "LocalProgram,",
        "NoControl,",
        "project_chain",
        "project, project_ref,",
        "project,\n        with_policy,",
        "typestate::{JumpReason, LocalAction, PassiveArmNavigation, PhaseCursor}",
        "pub mod steps {",
    ] {
        assert!(
            !program_head.contains(forbidden),
            "substrate::program root must not re-export typestate/internal helper: {forbidden}"
        );
    }

    for required in ["RoleProgram", "project", "MessageSpec", "StaticControlDesc"] {
        assert!(
            program_head.contains(required),
            "substrate::program root must stay on projection + descriptor SPI only: {required}"
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
        "export TOOLCHAIN=\"${TOOLCHAIN:-1.95.0}\"",
        "check_public_surface_budget.sh",
        "check_surface_hygiene.sh",
        "cargo +\"${TOOLCHAIN}\" test -p hibana --test root_surface --features std",
        "cargo +\"${TOOLCHAIN}\" test -p hibana --test substrate_surface --features std",
        "cargo +\"${TOOLCHAIN}\" test -p hibana --test public_surface_guards --features std",
        "cargo +\"${TOOLCHAIN}\" test -p hibana --test docs_surface --features std",
        "stable public API check passed",
    ] {
        assert!(
            script.contains(required),
            "crate-local public API gate must run the Rust 1.95 surface verifier: {required}"
        );
    }

    for forbidden in [
        "target/doc/hibana.json",
        "rustup which cargo --toolchain nightly",
        "rustup which rustc --toolchain nightly",
        "rustup which rustdoc --toolchain nightly",
        "-Z unstable-options",
        "HIBANA_RUSTDOC_JSON",
    ] {
        assert!(
            !script.contains(forbidden),
            "crate-local public API gate must not depend on nightly rustdoc JSON: {forbidden}"
        );
    }
}
