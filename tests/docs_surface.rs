use std::fs;
use std::path::{Path, PathBuf};

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
        .filter(|path| {
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                return false;
            };
            path.extension().and_then(|ext| ext.to_str()) == Some("rs")
                && name != "tests.rs"
                && !name.ends_with("_tests.rs")
        })
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

fn endpoint_facade_source() -> String {
    let mut source = read("src/endpoint.rs");
    source.push_str(&read_dir_rs("src/endpoint"));
    source
}

fn assert_absent(readme: &str, forbidden: &str, why: &str) {
    assert!(!readme.contains(forbidden), "{why}: {forbidden}");
}

fn collect_source_files(root: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(root)
        .unwrap_or_else(|err| panic!("read_dir {} failed: {}", root.display(), err))
    {
        let entry =
            entry.unwrap_or_else(|err| panic!("read_dir entry {} failed: {}", root.display(), err));
        let path = entry.path();
        if path.is_dir() {
            collect_source_files(&path, out);
        } else if path.extension().and_then(|ext| ext.to_str()) != Some("stderr") {
            out.push(path);
        }
    }
}

#[test]
fn readme_stays_self_contained_and_hibana_scoped() {
    let readme = read("README.md");

    for required in [
        "hibana-header.svg",
        "## What Hibana Is",
        "## Install",
        "## Quick Start",
        "## Application Guide",
        "## Protocol Integration",
        "## Guarantees",
        "## Validation",
        "cargo add hibana",
        "The default feature set is empty.",
        "flow().send() / recv() / offer() / RouteBranch::decode()",
        "`flow().send()`, `recv()`, or `RouteBranch::decode()` succeeds",
        "If you are writing an application, stay on `hibana::g` and `Endpoint`.",
        "are implementing a protocol crate, use `hibana::integration`",
        "Keep choreography terms local.",
        "### Branching, Resolvers, And Receive Evidence",
        "Route choice is a protocol fact, not a transport guess.",
        "Do not model route selection as a self-send control message.",
        "`offer()` and",
        "`RouteBranch::decode()` require framed descriptor-checked evidence.",
        "Protocol crates use the same `hibana::g` language as applications.",
        "no second composition language.",
        "let program = g::seq(prefix, app);",
        "let client: RoleProgram<0> = project(&program);",
        "let server: RoleProgram<1> = project(&program);",
        "let endpoint = rv.session(SessionId::new(1)).role(&client).enter()?;",
        "integration::runtime::Config::from_resources(...)",
        "integration::SessionKitStorage::uninit().init()",
        "kit.rendezvous(...)",
        "registered rendezvous .session(...).role(...)",
        "`integration::wire::{Payload, WireEncode, WirePayload}`",
        "fn decode_validated_payload(input: Payload<'_>) -> Self::Decoded<'_>",
        "`integration::ids::{EffIndex, SessionId}`",
        "`integration::runtime::TapEvent`",
        "cargo +1.95.0 check --no-default-features --lib -p hibana",
        "cargo +1.95.0 check --features std --lib -p hibana",
        "cargo +1.95.0 doc -p hibana --no-deps --no-default-features",
        "The full test suite is repository-only",
        "source-tree fixtures that",
        "intentionally excluded from the production crate package",
        "bash ./.github/scripts/run_final_form_gates.sh",
        "repo-only unit tests are enabled",
        "`hibana_repo_tests`",
        "It is intentionally kept outside the crate package.",
    ] {
        assert!(
            readme.contains(required),
            "README must stay self-contained and hibana-scoped: {required}"
        );
    }

    for forbidden in [
        "## Constitution",
        "Phase 7",
        "Phase 0a",
        ".github/scripts/check_",
        "final-form",
        "quarantine",
        "route frontier",
        "`WireDecode`",
        "owned default path",
        "hibana-quic",
        "hibana_mgmt",
        "hibana-mgmt",
        "hibana_epf",
        "hibana-epf",
        "hibana-cross-repo",
        "`hibana::integration::mgmt`",
        "`hibana::integration::policy::epf`",
        "`hibana::integration::mgmt::request_reply::PREFIX`",
        "`hibana::integration::mgmt::observe_stream::PREFIX`",
        "`hibana::integration::mgmt::ROLE_CONTROLLER`",
        "`hibana::integration::mgmt::ROLE_CLUSTER`",
        "`hibana::integration::mgmt::Request::Load(LoadRequest)`",
        "`hibana::integration::mgmt::Request::LoadAndActivate(LoadRequest)`",
        "`hibana::integration::mgmt::Request::Activate(SlotRequest)`",
        "`hibana::integration::mgmt::Request::Restore(SlotRequest)`",
        "`hibana::integration::mgmt::Request::Stats(SlotRequest)`",
        "`integration/cross-repo/`",
        "staging location for cross-repo smoke",
        "App code writes `APP: g::Program<_>`",
        "transport_prefix",
        "appkit",
        "appkits",
        "appkit_prefix",
        "build_management_prefix",
        "drive_management_pair",
        "MyDemux",
        "EPF",
        "project(&PROGRAM)",
        "const APP: g::Program<_>",
        "static APP: g::Program<_>",
        "const PROGRAM: g::Program<_>",
        "static PROGRAM: g::Program<_>",
        "`hibana::integration::program::steps`",
        "AUTO_MINT_WIRE",
        "enter(None)",
        "Passing `None`",
        "`CapDelegate`: `input[0] = (dst_rv << 16) | dst_lane`",
        "integration::SessionKit::enter(...)",
        "integration::policy::replay::PolicyAttrs",
        "integration::advanced::policy::replay::PolicyAttrs",
        "kit.enter::<",
        "fn decode_payload(input: Payload<'_>) -> Result<Self::Decoded<'_>, CodecError>",
        "cargo +1.95.0 test -p hibana --features std",
    ] {
        assert_absent(
            &readme,
            forbidden,
            "README must not leak other-crate or internal-only wording",
        );
    }

    assert_absent(
        &readme,
        &["project::", "<"].concat(),
        "README must not leak other-crate or internal-only wording",
    );

    for forbidden in ["cargo +nightly", "cargo +stable", "workspace_smoke"] {
        assert_absent(
            &readme,
            forbidden,
            "README must not pin removed toolchain or smoke-helper lanes",
        );
    }
}

#[test]
fn docs_do_not_regrow_stale_attach_api() {
    for path in [
        "README.md",
        "src/lib.rs",
        "src/integration.rs",
        "src/rendezvous/core.rs",
    ] {
        let source = read(path);
        for forbidden in [
            concat!("SessionKit::", "new"),
            "SessionKit::enter",
            "kit.enter::<",
            "enter(rv, sid",
            "from_resources(\n//!     &mut tap_buf,\n//!     &mut slab",
        ] {
            assert!(
                !source.contains(forbidden),
                "{path} must document the witness-chain attach API, not stale `{forbidden}`"
            );
        }
    }
}

#[test]
fn public_docs_do_not_expose_internal_storage_vocabulary() {
    for path in [
        "README.md",
        "src/lib.rs",
        "src/integration.rs",
        ".github/allowlists/lib-public-api.txt",
        ".github/allowlists/g-public-api.txt",
        ".github/allowlists/endpoint-public-api.txt",
        ".github/allowlists/integration-public-api.txt",
    ] {
        let source = read(path);
        for forbidden in ["resident", "Resident"] {
            assert!(
                !source.contains(forbidden),
                "{path} must keep resident descriptor/storage vocabulary internal: {forbidden}"
            );
        }
    }
}

#[test]
fn canonical_docs_are_readme_and_crate_docs_only() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    assert!(
        !root.join("docs").exists(),
        "docs/ must not regrow as a second canonical documentation tree"
    );
    assert!(
        !root.join("GUIDE.md").exists() && !root.join("INTERNALS.md").exists(),
        "standalone guide/internal docs must not regrow as second documentation authorities"
    );

    let readme = read("README.md");
    let endpoint = endpoint_facade_source();
    let lib = read("src/lib.rs");

    for (path, source) in [("README.md", readme.as_str()), ("src/lib.rs", lib.as_str())] {
        assert!(
            !source.contains("hibana::substrate"),
            "{path} must document the current integration surface, not stale substrate paths"
        );
        assert!(
            source.contains("hibana::integration"),
            "{path} must name the current integration surface"
        );
    }

    assert!(
        !readme.contains("preview restash on decode failure"),
        "README must not describe decode failure as a restashable preview"
    );
    assert!(
        endpoint.contains("A decode failure is terminal for the current generation"),
        "crate docs must document terminal decode failure semantics"
    );
    assert!(
        endpoint.contains("when a send, receive, or route decode succeeds")
            && endpoint.contains("Successful sends, receives, and route decodes consume")
            && endpoint.contains("/// progress. Dropped send/route previews")
            && !endpoint.contains("when a send or route decode succeeds")
            && !endpoint.contains("Successful sends and route decodes consume progress"),
        "endpoint docs must include direct recv() as a committed progress operation"
    );
    assert!(
        readme.contains("`flow().send()`, `recv()`, or `RouteBranch::decode()` succeeds")
            && !readme.contains("Endpoint progress happens when a send or\ndecode succeeds"),
        "README progress contract must include recv(), not only send/decode"
    );
    assert!(
        lib.contains("successful sends, receives, and route decodes")
            && !lib.contains("successful `send()` and `decode()` consume"),
        "crate root docs must include direct recv() as a committed progress operation"
    );
    assert!(
        !readme.contains("type BorrowedBytes = &'static [u8];"),
        "README borrowed payload example must not imply a static frame borrow"
    );
}

#[test]
fn projection_constructor_stays_on_canonical_call_shape() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let forbidden = ["project::", "<"].concat();
    let mut files = vec![
        root.join("README.md"),
        root.join("src/lib.rs"),
        root.join("src/g.rs"),
        root.join("src/integration.rs"),
    ];

    collect_source_files(&root.join("tests"), &mut files);

    let mut offenders = Vec::new();
    for file in files {
        let src = fs::read_to_string(&file)
            .unwrap_or_else(|err| panic!("read {} failed: {}", file.display(), err));
        for (line_idx, line) in src.lines().enumerate() {
            if line.contains(&forbidden) {
                let rel = file.strip_prefix(&root).unwrap_or(file.as_path()).display();
                offenders.push(format!("{}:{}:{}", rel, line_idx + 1, line.trim()));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "projection must use the canonical `project(&program)` call shape:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn quality_gates_do_not_directly_execute_non_executable_scripts() {
    let workflow = read(".github/workflows/quality-gates.yml");

    let required = "bash ./.github/scripts/run_final_form_gates.sh";
    assert!(
        workflow.contains(required),
        "quality gates must use the final-form gate as the only script authority: {required}"
    );

    for forbidden in [
        "dtolnay/rust-toolchain",
        "toolchain: stable",
        "cargo test",
        "cargo check",
        "run: ./.github/scripts/check_plane_boundaries.sh",
        "check_topology_hygiene.sh",
        "check_text_integrity.sh",
    ] {
        assert!(
            !workflow.contains(forbidden),
            "quality gates must remain a thin final-form wrapper: {forbidden}"
        );
    }
}

#[test]
fn protocol_docs_keep_route_choice_and_receive_evidence_out_of_control_vocabulary() {
    let readme = read("README.md");

    for required in [
        "Prefer in-band choice",
        "non-message signal",
        "`integration::policy`",
        "`ReceivedFrame`",
        "`IngressEvidence`",
        "Payload shape, queue position, carrier id, and driver observations are never branch authority.",
    ] {
        assert!(
            readme.contains(required),
            "README route/evidence section must document the final-form branch authority: {required}"
        );
    }

    for forbidden in [
        "GenericCapToken",
        "WireControlKind",
        "WireControlEffect",
        "integration::cap",
        "WireControlKind>::LABEL",
        "const LABEL: u8 =",
        "const TAP_ID",
        "CUSTOM_WIRE_TAP_ID",
        "0x0300 + 124",
        "0x0300 + 90",
    ] {
        assert!(
            !readme.contains(forbidden),
            "README must not reintroduce public control vocabulary: {forbidden}"
        );
    }
}

#[test]
fn core_repo_keeps_cross_repo_harness_outside_tree() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("integration/cross-repo");
    assert!(
        !path.exists(),
        "cross-repo smoke must stay outside the hibana repo: {}",
        path.display()
    );
}

#[test]
fn readme_keeps_ingress_demux_under_transport() {
    let readme = read("README.md");
    let everyday = readme
        .split("Useful integration owners:")
        .nth(1)
        .and_then(|tail| tail.split("### Transport").next())
        .expect("README must keep everyday integration owners before transport details");

    assert!(
        readme.contains("Ingress demux state belongs inside the transport owner")
            && readme.contains("Headerless receive is only valid")
            && !readme.contains("integration::binding")
            && !readme.contains("IngressSlot")
            && !readme.contains("role(...).binding")
            && !everyday.contains("binding"),
        "README must teach transport-owned ingress demux, not a core binding API"
    );
}

#[test]
fn docs_route_protocol_invisible_liveness_to_transport_errors() {
    let readme = read("README.md");
    let lib_rs = read("src/lib.rs");

    for doc in [readme.as_str(), lib_rs.as_str()] {
        assert!(
            doc.contains("TransportError")
                && doc.contains("poll_send")
                && doc.contains("poll_recv")
                && doc.contains("transport"),
            "docs must place protocol-invisible carrier liveness in the transport error path"
        );
    }
}

#[test]
fn crate_root_docs_do_not_regrow_internal_buckets() {
    let lib_rs = read("src/lib.rs");

    for forbidden in [
        "mod epf;",
        "pub mod runtime;",
        "pub mod transport;",
        "pub mod observe;",
    ] {
        assert!(
            !lib_rs.contains(forbidden),
            "crate root must stay on the minimal app/integration surface: {forbidden}"
        );
    }
}

#[test]
fn crate_root_docs_keep_descriptor_first_control_story() {
    let lib_rs = read("src/lib.rs");

    for required in [
        "Branch choice is either an in-band protocol message",
        "Transport evidence is",
        "not route authority",
    ] {
        assert!(
            lib_rs.contains(required),
            "crate root docs must describe the descriptor/evidence route model: {required}"
        );
    }

    for forbidden in [
        "cancel pair, checkpoint/rollback, splice",
        "shot and permissions are embedded in the const metadata",
        "manages local state (lane/gen/cap/splice)",
    ] {
        assert!(
            !lib_rs.contains(forbidden),
            "crate root docs must not describe the removed control execution model: {forbidden}"
        );
    }
}
