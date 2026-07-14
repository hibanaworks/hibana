use std::fs;
use std::path::{Path, PathBuf};

fn read(path: &str) -> String {
    let root = PathBuf::from(option_env!("HIBANA_REPO_ROOT").unwrap_or(env!("CARGO_MANIFEST_DIR")));
    let full = root.join(path);
    fs::read_to_string(&full)
        .unwrap_or_else(|err| panic!("read {} failed: {}", full.display(), err))
}

fn read_dir_rs(path: &str) -> String {
    let root = PathBuf::from(option_env!("HIBANA_REPO_ROOT").unwrap_or(env!("CARGO_MANIFEST_DIR")))
        .join(path);
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

fn compact_ws(source: &str) -> String {
    source.split_whitespace().collect::<Vec<_>>().join(" ")
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
    let header = read("hibana-header.svg");
    let searchable_readme = compact_ws(&readme);

    for required in [
        "hibana-header.svg",
        "alt=\"HIBANA - Choreography-Derived Runtime Enforcement for Rust\"",
        "## Quick Start",
        "## Model",
        "## Protocol Language",
        "## Runtime Boundary",
        "## Guarantees",
        "## Measured Footprint",
        "## Verification",
        "## Scope",
        "Hibana lets Rust programs execute a finite multiparty protocol",
        "choreography-derived runtime enforcement kernel",
        "cargo add hibana",
        "requires stable Rust `1.95` or newer",
        "cargo run --example ping_pong",
        "https://docs.rs/hibana",
        "[`examples/ping_pong.rs`](examples/ping_pong.rs)",
        "the same public API on hosted and embedded targets",
        "### Messages And Payloads",
        "### Endpoint Operations",
        "### Route Choice",
        "### Parallel And Repeated Regions",
        "### Transport",
        "### External Route Resolvers",
        "### Sessions, Reconfiguration, And Observation",
        "### Failure Semantics",
        "### When Deadlock Freedom Holds",
        "[`examples/pico/src/lib.rs`](examples/pico/src/lib.rs)",
        "| Modeled runtime SRAM envelope | 5,920 B | 8,954 B |",
        "| Runtime operation stack high-water | 2,831 B | 3,663 B |",
        "| Largest linked artifact in the tracked protocol matrix | 1,852 B | 16,384 B |",
        "Component maxima may come from different shapes",
        "Application state, concrete transport buffers, executor state, interrupt",
        "bash ./.github/scripts/run_final_form_gates.sh",
        "bash ./.github/scripts/check_kani.sh",
        "cargo +1.95.0 check --manifest-path examples/pico/Cargo.toml",
        "Successful projection alone is not a distributed deadlock-freedom guarantee.",
        "eventually delivers each accepted frame or reports terminal closure",
        "The [Lean proof boundary](https://github.com/hibanaworks/hibana/blob/main/proofs/lean/README.md)",
        "Cross-binary agreement is defined over canonical wire schemas",
    ] {
        assert!(
            searchable_readme.contains(required),
            "README must explain the canonical Hibana surface and boundary: {required}"
        );
    }

    for forbidden in [
        "## Constitution",
        "Phase 7",
        "Phase 0a",
        "final-form",
        "route frontier",
        "`WireDecode`",
        "g::Msg<L, P, K>",
        "Msg<L, P, K>",
        "owned default path",
        "hibana_mgmt",
        "hibana-mgmt",
        "hibana_epf",
        "hibana-epf",
        "hibana-cross-repo",
        "`hibana::runtime::mgmt`",
        "`hibana::runtime::resolver::epf`",
        "`runtime/cross-repo/`",
        "App code writes `APP: g::Program<_>`",
        "transport_prefix",
        "appkit",
        "appkits",
        "duplicate branch labels",
        "branch labels must be unique",
        "MyDemux",
        "project(&PROGRAM)",
        "const APP: g::Program<_>",
        "static APP: g::Program<_>",
        "const PROGRAM: g::Program<_>",
        "static PROGRAM: g::Program<_>",
        "`hibana::runtime::program::steps`",
        "AUTO_MINT_WIRE",
        "enter(None)",
        "Passing `None`",
        "runtime::SessionKit::enter(...)",
        "runtime::resolver::replay::ResolverAttrs",
        "runtime::advanced::resolver::replay::ResolverAttrs",
        "kit.enter::<",
        "fn decode_payload(input: Payload<'_>) -> Result<Self::Decoded<'_>, CodecError>",
        "project::<",
        "cargo +nightly",
        "cargo +stable",
        "workspace_smoke",
        "Pico-class",
        "Pico class",
        "### Research Context",
        "research direction",
        "Novelty is a research claim",
        "assumption_indexed_epoch_erased_byte_exact_end_to_end_refinement",
        "Mediated -> Authentic -> Ordered -> Closing -> Fair",
        "### Elastic Re-entry And Erasure",
        "### Cross-tool Evidence",
        "### What Hibana Does Not Claim",
        "message-erased",
        "localside",
        "Localside",
        "#guarantees-and-assumptions",
        "`GlobalFairnessAssumptions`",
        "`CarrierProfile`",
        "`RustKernelRefinement`",
        "assumption-indexed",
        "epoch-erased",
        "Novelty",
        "world first",
        "```rust,ignore",
        "<!-- ping-pong-example:",
    ] {
        assert_absent(
            &readme,
            forbidden,
            "README must not publish removed or internal-only vocabulary",
        );
    }

    assert!(
        header.contains("Choreography-Derived Runtime Enforcement for Rust")
            && header.contains("Compact descriptors")
            && !header.contains("Session-Typed Choreographic Programming"),
        "README header must use the same public positioning as the README"
    );
}

#[test]
fn onboarding_links_one_gated_runnable_example_without_copying_it() {
    let readme = read("README.md");
    let searchable_readme = compact_ws(&readme);
    let quick_start = readme
        .find("## Quick Start")
        .expect("README must start with installation and execution");
    let model = readme
        .find("## Model")
        .expect("README must explain the model after quick start");
    let language = readme
        .find("## Protocol Language")
        .expect("README must explain the protocol language");
    let runtime = readme
        .find("## Runtime Boundary")
        .expect("README must explain runtime integration");
    let guarantees = readme
        .find("## Guarantees")
        .expect("README must separate kernel guarantees from integration requirements");
    let footprint = readme
        .find("## Measured Footprint")
        .expect("README must quantify the embedded resource envelope");
    let verification = readme
        .find("## Verification")
        .expect("README must explain verification evidence");
    let scope = readme
        .find("## Scope")
        .expect("README must close with its exact claim boundary");

    assert!(
        quick_start < model
            && model < language
            && language < runtime
            && runtime < guarantees
            && guarantees < footprint
            && footprint < verification
            && verification < scope,
        "README must move from execution through model and boundary to evidence and scope"
    );

    let runnable_section = &readme[quick_start..model];
    for required in [
        "cargo add hibana",
        "cargo run --example ping_pong",
        "examples/ping_pong.rs",
        "ping=7, pong=8",
        "fn ping_pong() -> (RoleProgram<0>, RoleProgram<1>)",
        "g::send::<0, 1, Msg<1, u32>>()",
        "g::send::<1, 0, Msg<2, u32>>()",
        "(project(&choreography), project(&choreography))",
    ] {
        assert!(
            runnable_section.contains(required),
            "install section must use the runnable canonical example: {required}"
        );
    }
    assert!(
        !runnable_section.contains("fn main()")
            && !runnable_section.contains("#[path =")
            && runnable_section.matches("```rust\n").count() == 1,
        "Quick Start must show one concise protocol excerpt and link the executable source"
    );

    let example = read("examples/ping_pong.rs");
    assert!(
        example.contains("fn main()")
            && example.contains("assert_eq!((ping, pong), (7, 8))")
            && example.contains("println!(\"ping={ping}, pong={pong}\")"),
        "ping_pong must remain a self-checking executable"
    );

    let thumbv6m_manifest = read("examples/pico/Cargo.toml");
    let thumbv6m_example = read("examples/pico/src/lib.rs");
    assert!(
        thumbv6m_manifest.contains("hibana = { path = \"../..\", default-features = false }")
            && thumbv6m_example.starts_with("#![no_std]")
            && thumbv6m_example
                .contains("pub fn projected_pair() -> (RoleProgram<0>, RoleProgram<1>)")
            && thumbv6m_example.contains("g::send::<0, 1, Msg<1, u32>>()")
            && thumbv6m_example.contains("g::send::<1, 0, Msg<2, u32>>()"),
        "the tracked thumbv6m example must compile the canonical public projection surface"
    );
    assert!(
        readme.contains("[`examples/pico/src/lib.rs`](examples/pico/src/lib.rs)")
            && readme.contains("`thumbv6m-none-eabi`")
            && searchable_readme.contains("target-only Hibana API"),
        "README must tie the measured footprint to the tracked no_std public-API example"
    );

    let crate_docs = read("src/lib.rs");
    assert!(
        crate_docs.contains("cargo run --example ping_pong")
            && !crate_docs.contains("endpoint.send::<g::Msg<1, u32>>"),
        "crate docs must route onboarding to the executable example"
    );

    let final_gate = read(".github/scripts/run_final_form_gates.sh");
    assert!(
        final_gate.contains("cargo +\"${TOOLCHAIN}\" run --quiet --example ping_pong")
            && final_gate.contains("ping_pong example output mismatch"),
        "the release gate must execute and validate the onboarding example"
    );

    let package_gate = read(".github/scripts/check_package_artifact.sh");
    for required in ["examples/ping_pong.rs", "examples/support/in_memory.rs"] {
        assert!(
            package_gate.contains(required),
            "published package must contain the runnable example: {required}"
        );
    }

    let no_std_gate = read(".github/scripts/check_no_std_build.sh");
    assert!(
        no_std_gate.contains("--manifest-path examples/pico/Cargo.toml")
            && no_std_gate.matches("--target thumbv6m-none-eabi").count() == 2
            && no_std_gate.contains("projection-example=1"),
        "the no_std gate must compile both Hibana and its tracked projection example for thumbv6m"
    );
}

#[test]
fn readme_covers_role_protocol_and_integration_without_a_second_guide() {
    let readme = read("README.md");
    let searchable = compact_ws(&readme);

    for required in [
        "`WireEncode::encode_into` writes deterministic bytes",
        "`WirePayload::validate_payload` accepts the exact canonical byte shape",
        "`WirePayload::decode_validated_payload` decodes bytes already validated",
        "async fn client(endpoint: &mut Endpoint<'_, 0>)",
        "async fn receive_choice(",
        "Dropping an unpolled send publishes no progress.",
        "The reverse call order is unavailable",
        "`RendezvousKit::enter(...)` attaches a projected role",
        "implementations provide two associated handle types and five operations",
        "`open(PortOpen)`",
        "`poll_send(tx, Outgoing, cx)`",
        "`cancel_send(tx)`",
        "`poll_recv(rx, cx)`",
        "`requeue(rx)`",
        "`ReceivedFrame::deterministic",
        "`ReceivedFrame::framed",
        "session id (4 bytes, big endian) | lane | source role | target role | frame label",
        "let state = RouteState { accept: true };",
        "rendezvous.set_resolver(&role_program, resolver)?;",
        "Distinct `SessionId` values have independent cursors",
        "`RendezvousKit::tap()` returns a read-only iterator",
        "There is no public same-generation retry, reselection, timeout, or cancellation",
        "Each public boundary reports its own error instead of widening all failures",
        "| `rendezvous` / `enter` | `AttachError` |",
        "| `set_resolver` / resolver callback | `ResolverError` |",
        "| `poll_send` / `poll_recv` / `requeue` | `TransportError` |",
        "| `send` / `recv` / `offer` / branch operation | `EndpointError` |",
        "## Build And Release",
    ] {
        assert!(
            searchable.contains(required),
            "README must remain a complete public guide: {required}"
        );
    }

    assert!(
        !readme.contains("```rust,ignore")
            && !readme.contains("See the full contract")
            && !readme.contains("documented on [`Transport`"),
        "README must contain concrete public examples and must not outsource missing contracts"
    );

    let documented_repo_gates = readme
        .lines()
        .filter(|line| line.starts_with("bash ./.github/scripts/"))
        .collect::<Vec<_>>();
    assert_eq!(
        documented_repo_gates,
        [
            "bash ./.github/scripts/run_final_form_gates.sh",
            "bash ./.github/scripts/check_kani.sh",
        ],
        "README must expose only the two complete release gates, not internal phase scripts"
    );
}

#[test]
fn docs_do_not_regrow_forbidden_attach_api() {
    for path in [
        "README.md",
        "src/lib.rs",
        "src/runtime.rs",
        "src/rendezvous/core.rs",
    ] {
        let source = read(path);
        for forbidden in [
            "SessionKit::new",
            "SessionKit::enter",
            "kit.enter::<",
            "enter(rv, sid",
            "Config::from_resources",
            "runtime::Config",
            "CounterClock",
            "RING_EVENTS",
            "TAP_EVENTS",
            "tap_buf",
        ] {
            assert!(
                !source.contains(forbidden),
                "{path} must document the witness-chain attach API, not forbidden `{forbidden}`"
            );
        }
    }
}

#[test]
fn public_docs_do_not_expose_internal_storage_vocabulary() {
    for path in [
        "README.md",
        "src/lib.rs",
        "src/runtime.rs",
        "src/runtime/session_kit.rs",
        ".github/allowlists/lib-public-api.txt",
        ".github/allowlists/g-public-api.txt",
        ".github/allowlists/endpoint-public-api.txt",
        ".github/allowlists/runtime-public-api.txt",
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
fn transport_docs_do_not_reference_private_runtime_storage() {
    let transport = read("src/transport.rs");
    for forbidden in [
        "runtime_core::config::Config::slab",
        "runtime_core::resources::RuntimeResources::slab",
        "Config::slab",
        "provides a slab",
    ] {
        assert!(
            !transport.contains(forbidden),
            "transport docs must not point implementors at private runtime storage: {forbidden}"
        );
    }
}

#[test]
fn canonical_docs_are_readme_and_crate_docs_only() {
    let root = PathBuf::from(option_env!("HIBANA_REPO_ROOT").unwrap_or(env!("CARGO_MANIFEST_DIR")));
    assert!(
        !root.join("docs").exists(),
        "docs/ must not regrow as a second canonical documentation tree"
    );
    assert!(
        !root.join("GUIDE.md").exists() && !root.join("INTERNALS.md").exists(),
        "standalone guide/internal docs must not regrow as second documentation authorities"
    );

    let readme = read("README.md");
    let searchable_readme = compact_ws(&readme);
    let endpoint = endpoint_facade_source();
    let lib = read("src/lib.rs");

    for (path, source) in [("README.md", readme.as_str()), ("src/lib.rs", lib.as_str())] {
        assert!(
            !source.contains("hibana::substrate"),
            "{path} must document the current runtime surface, not forbidden substrate paths"
        );
        assert!(
            source.contains("hibana::runtime"),
            "{path} must name the current runtime surface"
        );
        assert!(
            !source.to_ascii_lowercase().contains("localside"),
            "{path} must use standard role-local terminology"
        );
    }

    assert!(
        !readme.contains("preview restash on decode failure"),
        "README must not describe decode failure as a restashable preview"
    );
    assert!(
        endpoint.contains("A committed receive fault poisons the session generation"),
        "crate docs must document terminal receive failure semantics"
    );
    assert!(
        endpoint.contains("route branch first-step\n//! operation succeeds")
            && endpoint.contains("route branch first-step operations consume")
            && endpoint
                .contains("//! progress. Dropped unpolled sends do not publish runtime progress")
            && !endpoint.contains("when a send or route recv succeeds")
            && !endpoint.contains("Successful sends and route recvs consume progress"),
        "endpoint docs must include branch first-step operations as committed progress"
    );
    assert!(
        searchable_readme.contains("`recv()`, or a route branch first-step operation succeeds")
            && !readme.contains("Endpoint progress happens when a send or\ndecode succeeds"),
        "README progress contract must include route branch first-step operations"
    );
    assert!(
        lib.contains("route branch first-step operations")
            && !lib.contains("successful `send()` and `decode()` consume"),
        "crate root docs must include branch first-step operations as committed progress"
    );
    assert!(
        !readme.contains("type BorrowedBytes = &'static [u8];"),
        "README borrowed payload example must not imply a static frame borrow"
    );
}

#[test]
fn projection_constructor_stays_on_canonical_call_shape() {
    let root = PathBuf::from(option_env!("HIBANA_REPO_ROOT").unwrap_or(env!("CARGO_MANIFEST_DIR")));
    let forbidden = "project::<";
    let mut files = vec![
        root.join("README.md"),
        root.join("src/lib.rs"),
        root.join("src/g.rs"),
        root.join("src/runtime.rs"),
    ];

    collect_source_files(&root.join("tests"), &mut files);

    let mut offenders = Vec::new();
    for file in files {
        if file
            .strip_prefix(&root)
            .map(|relative| relative == Path::new("tests/docs_surface.rs"))
            .unwrap_or(false)
        {
            continue;
        }
        let src = fs::read_to_string(&file)
            .unwrap_or_else(|err| panic!("read {} failed: {}", file.display(), err));
        for (line_idx, line) in src.lines().enumerate() {
            if line.contains(forbidden) {
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
    let searchable_readme = compact_ws(&readme);
    let transport = read("src/transport.rs");

    for required in [
        "Prefer in-band choice",
        "non-message signal",
        "`ResolverRef::<ID>::decision_state(...)`",
        ".roll()",
        "Payload contents, queue position, and carrier observations are never branch authority.",
        "`RouteBranch::label()` then reports that selected arm's first logical message label",
        "Resolver failure rejects the step; it does not select another arm.",
    ] {
        assert!(
            searchable_readme.contains(required),
            "README route/evidence section must document the final-form branch authority: {required}"
        );
    }
    for required in [
        "Evidence is descriptor input, not route authority. `Deterministic` is valid",
        "only for direct receives, or after a route branch has already materialized a",
        "Offer and unresolved route demux paths must",
        "pub const fn deterministic(payload: Payload<'f>)",
        "pub const fn framed(header: FrameHeader, payload: Payload<'f>)",
    ] {
        assert!(
            transport.contains(required),
            "Transport rustdoc must own receive-evidence construction detail: {required}"
        );
    }

    for forbidden in [
        "runtime::cap",
        "const LABEL: u8 =",
        "const TAP_ID",
        "CUSTOM_WIRE_TAP_ID",
        "0x0300 + 124",
        "0x0300 + 90",
        "`IngressEvidence`",
        "duplicate labels",
        "duplicate branch labels",
        "branch labels must be unique",
        "duplicate branch labels rejected",
        "label is branch authority",
        "selected choreography branch label",
        "branch.send::<g::Msg<33, ()>>(&()).await?",
    ] {
        assert!(
            !readme.contains(forbidden),
            "README must not contain forbidden public vocabulary: {forbidden}"
        );
    }
}

#[test]
fn core_repo_keeps_cross_repo_harness_outside_tree() {
    let path = PathBuf::from(option_env!("HIBANA_REPO_ROOT").unwrap_or(env!("CARGO_MANIFEST_DIR")))
        .join("runtime/cross-repo");
    assert!(
        !path.exists(),
        "cross-repo smoke must stay outside the hibana repo: {}",
        path.display()
    );
}

#[test]
fn readme_keeps_ingress_demux_under_transport() {
    let readme = read("README.md");
    let transport = read("src/transport.rs");

    assert!(
        readme
            .contains("`Transport` owns byte buffers, framing, readiness, ingress demultiplexing")
            && transport.contains("demultiplex received frames before returning")
            && !readme.contains("runtime::binding")
            && !readme.contains("IngressSlot")
            && !readme.contains("role(...).binding"),
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

    for forbidden in ["mod epf;", "pub mod transport;", "pub mod observe;"] {
        assert!(
            !lib_rs.contains(forbidden),
            "crate root must stay on the minimal app/runtime surface without internal buckets: {forbidden}"
        );
    }
}

#[test]
fn crate_root_docs_keep_descriptor_first_control_story() {
    let lib_rs = read("src/lib.rs");

    for required in [
        "Branch choice is either an in-band protocol message",
        "projected first",
        "visible endpoint operation confirmed by descriptor-checked receive evidence",
        "Transport evidence is",
        "not route authority",
        "route controller mismatch",
        "## Boundary contract",
        "ambiguous simultaneous endpoint",
        "receives a message after descriptor evidence matches",
    ] {
        assert!(
            lib_rs.contains(required),
            "crate root docs must describe the descriptor/evidence route model: {required}"
        );
    }

    for forbidden in [
        "cancel pair, checkpoint/restore, splice",
        "duplicate branch labels",
        "branch labels must be unique",
        "## Guarantees",
        "receives a deterministic message",
        "overlapping `(role, lane)`",
        "shot and permissions are embedded in the const metadata",
        "manages local state (lane/gen/cap/splice)",
    ] {
        assert!(
            !lib_rs.contains(forbidden),
            "crate root docs must not describe the forbidden execution model: {forbidden}"
        );
    }
}
