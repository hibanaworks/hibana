use std::fs;
use std::path::{Path, PathBuf};

fn read_plain(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|err| panic!("read {} failed: {}", path.display(), err))
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
    let mut source = String::new();
    for part in parts {
        source.push_str(
            &fs::read_to_string(&part)
                .unwrap_or_else(|err| panic!("read {} failed: {}", part.display(), err)),
        );
    }
    source
}

fn lib_rs() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs");
    read_plain(&path)
}

fn endpoint_rs() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/endpoint.rs");
    read_plain(&path)
}

fn global_rs() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/global.rs");
    read_plain(&path)
}

fn runtime_rs() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/runtime.rs");
    let mut source = read_plain(&path);
    source.push_str(&read_dir_rs("src/runtime"));
    source
}

fn g_rs() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/g.rs");
    read_plain(&path)
}

fn flow_rs() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/endpoint/flow.rs");
    read_plain(&path)
}

fn role_program_rs() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/global/role_program.rs");
    read_plain(&path)
}

fn public_api_script_rs() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".github/scripts/check_hibana_public_api.sh");
    read_plain(&path)
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
    let runtime_rs = runtime_rs();
    let g_rs = g_rs();
    let flow_rs = flow_rs();
    let role_program_rs = role_program_rs();
    let g_ws = compact_ws(&g_rs);
    let program_head = {
        let start = runtime_rs
            .find("pub mod program {")
            .expect("runtime program surface block must exist");
        let rest = &runtime_rs[start..];
        let end = rest
            .find("/// Protocol-neutral identifiers")
            .expect("program surface block must end before id surface");
        &rest[..end]
    };

    assert!(
        lib_rs.contains("pub mod g;"),
        "hibana root must expose g surface"
    );
    assert!(
        g_ws.contains("pub struct Program<Steps>")
            && g_ws.contains("pub struct Msg<const LOGICAL_LABEL: u8, Payload>")
            && !g_ws.contains(&["pub mod ", "con", "trol"].concat())
            && !g_ws.contains("pub struct Msg<const LOGICAL_LABEL: u8, Payload, Control")
            && g_ws.contains("pub struct Send<const FROM: u8, const TO: u8, M>")
            && !g_ws.contains("pub struct Send<const FROM: u8, const TO: u8, M, const LANE")
            && g_ws.contains("pub struct Seq<Left, Right>")
            && g_ws.contains("pub struct Route<Left, Right>")
            && g_ws.contains("pub struct Par<Left, Right>")
            && g_ws.contains("pub struct Roll<Inner>")
            && g_ws.contains("pub struct Resolve<Inner, const RESOLVER_ID: u16>")
            && !g_ws.contains("pub(crate) struct Resolver<Inner, const RESOLVER_ID: u16>")
            && !g_ws.contains("pub struct Resolver<Inner, const RESOLVER_ID: u16>")
            && g_ws.contains("pub const fn send<const FROM: u8, const TO: u8, M>()")
            && !g_ws.contains("pub const fn send<const FROM: u8, const TO: u8, M, const LANE")
            && g_ws.contains("pub const fn seq<LeftSteps, RightSteps>(")
            && g_ws.contains("pub const fn route<LeftSteps, RightSteps>(")
            && g_ws.contains("pub const fn par<LeftSteps, RightSteps>(")
            && g_ws.contains("pub const fn roll(self)"),
        "hibana::g root must stay on named canonical app primitives"
    );
    assert!(
        !g_ws.contains("pub use crate::global::{par, route, send, seq};"),
        "hibana::g combinators must not be re-exported from the lower global substrate"
    );
    assert!(
        !g_ws.contains("advanced") && !global_rs.contains("pub mod advanced {"),
        "hibana::g must not expose protocol-implementor SPI"
    );
    assert!(
        lib_rs.contains("pub mod runtime;"),
        "hibana root must expose runtime surface"
    );
    assert!(
        lib_rs.contains(
            "pub use endpoint::{Endpoint, EndpointError, EndpointResult, Flow, RouteBranch};"
        ),
        "hibana root must expose endpoint core API"
    );

    for (owner, source) in [
        ("lib.rs", lib_rs.as_str()),
        ("global.rs", global_rs.as_str()),
        ("runtime.rs", runtime_rs.as_str()),
        ("role_program.rs", role_program_rs.as_str()),
    ] {
        assert!(
            !source.contains(&["#[", "allow(", "private_bounds", ")]"].concat())
                && !source.contains("#[expect(\n        private_bounds")
                && !source.contains("#[expect(private_bounds"),
            "{owner} must not rely on private_bounds lint allowance in the public surface"
        );
    }
    assert!(
        !flow_rs.contains("ErasedSendInput")
            && flow_rs.contains("pub fn send<'a>(")
            && flow_rs.contains("payload: &'a M::Payload")
            && flow_rs.contains("kernel::RawSendPayload::from_typed::<M::Payload>(payload)")
            && !flow_rs.contains("Into<Option<&'a M::Payload>>")
            && !flow_rs.contains(".into()"),
        "Flow::send must stay a single required typed-payload API without optional or private-bound argument wrappers"
    );

    for forbidden in [
        "pub use global::{",
        "pub use ingress::{",
        "pub use session::brand::{",
        "pub use session::types::LaneId as Lane;",
        "pub use session::types::{Gen, LaneId, RendezvousId, SessionId};",
        "pub use endpoint::{ControlEmission, CursorEndpoint};",
        "pub use endpoint::{ControlOutcome, CursorEndpoint};",
        "pub use epf::Slot;",
        "pub use epf::TapEvent;",
        "pub use runtime::SessionKit;",
        "pub use runtime::config::{Clock, CounterClock};",
        "pub use runtime::consts::{",
        "pub mod global;",
        "pub mod session;",
        "pub mod transport;",
        "pub mod observe;",
        "pub use crate::global::{par_chain, route_chain};",
    ] {
        assert!(
            !lib_rs.contains(forbidden),
            "hibana root must not re-export non-canonical/internal runtime names: {forbidden}"
        );
    }

    for forbidden in ["par_chain", "route_chain"] {
        assert!(
            !g_rs.contains(forbidden),
            "hibana::g root must not expose hidden builder aliases: {forbidden}"
        );
    }

    for forbidden in [
        "pub fn resolver(",
        "pub const fn resolver(",
        " pub use crate::global::{Msg, Program, par, resolver, route, send, seq};",
    ] {
        assert!(
            !g_rs.contains(forbidden),
            "hibana::g root must not expose a top-level resolver helper: {forbidden}"
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
        concat!("AllowsEndpoint", "Mi", "nt"),
        concat!("Mi", "nt", "ConfigMarker"),
        concat!("Mi", "nt", "Config"),
        &["#[", "allow(", "private_bounds", ")]"].concat(),
        "#[expect(private_bounds)]",
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
            "app-facing flow surface must not mention forbidden resolver internals: {forbidden}"
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
                && !runtime_rs.contains(forbidden)
                && !g_rs.contains(forbidden),
            "forbidden token and cfg-test owners must not re-enter the public crate surface: {forbidden}"
        );
    }

    for forbidden in [
        concat!("Canonical", "Control"),
        concat!("External", "Control"),
        concat!("Control", "Handling"),
        concat!("Control", "Message", "Kind"),
        concat!("Control", "Message"),
        concat!("Loop", "Break", "Steps,"),
        "RouteResolver,",
        "const_dsl::{\n            ControlScopeKind, DynamicMeta,",
        "LocalProgram,",
        concat!("No", "Control,"),
        "project_chain",
        "project, project_ref,",
        "project,\n        with_resolver,",
        "typestate::{JumpReason, LocalAction, EventCursor}",
        concat!("Passive", "ArmNavigation"),
        "pub mod steps {",
    ] {
        assert!(
            !program_head.contains(forbidden),
            "runtime::program root must not re-export typestate/internal helper: {forbidden}"
        );
    }

    for required in ["RoleProgram", "project"] {
        assert!(
            program_head.contains(required),
            "runtime::program root must stay on projection + descriptor SPI only: {required}"
        );
    }
    assert!(
        !program_head.contains("Message"),
        "runtime::program root must not re-export app-facing message SPI"
    );

    for forbidden in [
        &["pub mod ", "con", "trol", " {"].concat(),
        "pub mod metadata {",
        "pub mod loops {",
        "pub mod typestate {",
        "project_ref",
        "route_chain",
        "par_chain",
    ] {
        assert!(
            !global_rs.contains(forbidden),
            "forbidden lower-layer helper must not remain in global surface source: {forbidden}"
        );
    }
}

#[test]
fn public_api_gate_tracks_g_and_runtime_surfaces() {
    let script = public_api_script_rs();

    for required in [
        "export TOOLCHAIN=\"${TOOLCHAIN:-1.95.0}\"",
        "check_public_surface_budget.sh",
        "check_surface_hygiene.sh",
        "cargo +\"${TOOLCHAIN}\" test -p hibana --test root_surface --features std",
        "cargo +\"${TOOLCHAIN}\" test -p hibana --test runtime_surface --features std",
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
