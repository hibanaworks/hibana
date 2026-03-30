use std::fs;
use std::path::PathBuf;

fn read(path: &str) -> String {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let full = root.join(path);
    fs::read_to_string(&full)
        .unwrap_or_else(|err| panic!("read {} failed: {}", full.display(), err))
}

fn contains_tokenish(haystack: &str, needle: &str) -> bool {
    haystack.match_indices(needle).any(|(idx, _)| {
        let before = haystack[..idx].chars().next_back();
        let after = haystack[idx + needle.len()..].chars().next();
        let is_boundary = |ch: Option<char>| {
            ch.is_none_or(|value| !(value.is_ascii_alphanumeric() || value == '_'))
        };
        is_boundary(before) && is_boundary(after)
    })
}

#[test]
fn public_docs_use_canonical_surface_paths() {
    let readme = read("README.md");
    let lib_rs = read("src/lib.rs");
    let cargo_toml = read("Cargo.toml");

    for source in [&readme, &lib_rs] {
        for forbidden in [
            "hibana::global::",
            "hibana::runtime::",
            "hibana::control::",
            "hibana::transport::",
            "hibana::observe::",
            "observe::TapEvent",
            "observe::for_each_since",
            "hibana::endpoint::CursorEndpoint",
            "hibana::g::policy(",
            "g::policy::",
            "g::project::<",
            "g::RoleProgram<",
            "hibana::g::with_policy(",
            "hibana::g::advanced::with_policy(",
            "hibana::g::route::<",
            "hibana::g::par::<",
            "hibana::g::route_chain(",
            "hibana::g::par_chain(",
            "PolicyMode::dynamic(",
            "epf::ops::",
            "mgmt_epf_observe.rs",
            "runtime::consts::",
            "`attach_cursor`",
            "handshake/interop",
            "interop-style end-to-end session driving",
            "cluster.attach_cursor(",
            "claim.attach_cursor(",
            "BindingSlot::map_lane(",
            "`map_lane()`",
            "NullBinding",
            "`binding::Binding`",
            "Incoming as IncomingClassification",
            "`SendMeta`",
            "type Cluster = SessionKit",
            "VmSlot",
            "VmHeader",
            "endpoint::delegate",
            "SessionKit::delegate_claim",
            "claim.enter(",
            "CONTROLLER_PROGRAM",
            "examples/tcp_tokio.rs",
            "examples/custom_binding.rs",
            "examples/transactional_session.rs",
            "examples/distributed_migration.rs",
            "examples/mgmt_epf_control.rs",
            "--example tcp_tokio",
            "--example custom_binding",
            "--example transactional_session",
            "--example distributed_migration",
            "--example mgmt_epf_control",
            "slot: /* hidden slot-scoped callback parameter */",
            "prefer_local_input: bool",
            "Program<_>",
            "Result<_, ()>",
            "NoopMetrics",
            "ContextValue::from_bool(",
            "mint_endpoint_token(",
            "OneShot",
            "ManyShot",
            "Result<DynamicResolution, ()>",
            "TRYBUILD=overwrite cargo test -p hibana --test ui --features std",
            "`test-utils`",
            "runtime::LABEL_",
            "substrate::runtime::LABEL_",
            "cargo test -p hibana\n",
        ] {
            assert!(
                !source.contains(forbidden),
                "public docs must not teach hidden legacy root paths: {forbidden}"
            );
        }
        assert!(
            !contains_tokenish(source, "binding::SendMeta"),
            "public docs must not teach the deleted SendMeta alias"
        );
    }

    for required in [
        "## App Surface",
        "App authors should stay on `g` and `Endpoint`.",
        "## Protocol-Implementor Walkthrough",
        "## Substrate Surface (protocol implementors only)",
        "Protocol implementors use the protocol-neutral SPI:",
        "### App Result and Error Types",
        "## Control Message Surface",
        "## Transport Seam",
        "## BindingSlot Contract",
        "## Policy Plane",
        "## Control Messages and Capability Kinds",
        "## Wire and Transport Observation",
        "## Management Session",
        "`hibana::SendResult<T>`",
        "`hibana::RecvResult<T>`",
        "`hibana::SendError`",
        "`hibana::RecvError`",
        "`hibana::RouteBranch`",
        "`hibana::substrate::CpError`",
        "`hibana::substrate::AttachError`",
        "`hibana::substrate::Transport`",
        "`hibana::substrate::binding::NoBinding`",
        "`hibana::substrate::policy::PolicySignalsProvider`",
        "`hibana::substrate::policy::epf::{Header, Slot}`",
        "`hibana::substrate::wire::{Payload, WireDecode, WireEncode}`",
        "`CanonicalControl<K>`",
        "`ExternalControl<K>`",
        "`MessageSpec`",
        "`ControlMessage`",
        "`ControlMessageKind`",
        "`Config::new(tap_buf, slab)`",
        "`Config::tap_storage()` and `Config::slab()`",
        "`Config::with_lane_range(range)`",
        "`ContextValue::{NONE, FALSE, TRUE}`",
        "`PolicyAttrs::new()`, `insert(id, value)`, and `query(id)`",
        "There is no public `g::splice`, `g::delegate`, or `g::reroute`.",
        "`RouteDecisionKind`, `LoopContinueKind`, `LoopBreakKind`",
        "`SpliceIntentKind`, `SpliceAckKind`, `RerouteKind`",
        "`PolicyLoadKind`, `PolicyActivateKind`, `PolicyRevertKind`, `PolicyAnnotateKind`",
        "`LoadBeginKind`, `LoadCommitKind`",
        "`Reply::Loaded(report)`",
        "`Reply::ActivationScheduled(report)`",
        "`Reply::Reverted(report)`",
        "`Reply::Stats { stats, staged_version }`",
        "`hibana::substrate::mgmt::Request::Load(LoadRequest)`",
        "`hibana::substrate::mgmt::Request::LoadAndActivate(LoadRequest)`",
        "`hibana::substrate::mgmt::Request::Activate(SlotRequest)`",
        "`hibana::substrate::mgmt::Request::Revert(SlotRequest)`",
        "`hibana::substrate::mgmt::Request::Stats(SlotRequest)`",
        "`LoadRequest`",
        "`SlotRequest`",
        "`hibana::substrate::mgmt::request_reply::PREFIX`",
        "`hibana::substrate::mgmt::observe_stream::PREFIX`",
        "`hibana::substrate::mgmt::ROLE_CONTROLLER`",
        "`hibana::substrate::mgmt::ROLE_CLUSTER`",
        "`hibana::substrate::mgmt::tap::TapEvent`",
        "`SessionKit::enter(...)`",
        "`flow().send()`, `recv()`, `offer()`, and `decode()`",
        "`compose::seq`",
        "Dynamic policy remains explicit:",
        "there is no public VM-run API separate from the resolver/policy surface",
        "`BindingSlot` is demux and transport observation only. It does not decide route arms.",
        "heap-backed lower-layer storage",
        "`no_alloc` oriented in",
        "bash ./.github/scripts/check_hibana_public_api.sh",
        "bash ./.github/scripts/check_policy_surface_hygiene.sh",
        "bash ./.github/scripts/check_surface_hygiene.sh",
        "bash ./.github/scripts/check_lowering_hygiene.sh",
        "bash ./.github/scripts/check_boundary_contracts.sh",
        "bash ./.github/scripts/check_warning_free.sh",
        "bash ./.github/scripts/check_direct_projection_binary.sh",
        "bash ./.github/scripts/check_no_std_build.sh",
        "cargo check --all-targets -p hibana",
        "cargo test -p hibana --features std",
        "cargo test -p hibana --test ui --features std",
        "cargo test -p hibana --test policy_replay --features std",
    ] {
        assert!(
            readme.contains(required),
            "README must document the canonical app/substrate split and validation flow: {required}"
        );
    }

    for forbidden in [
        "`hibana::substrate::mgmt::session::Request::Load(LoadRequest)`",
        "`hibana::substrate::mgmt::session::Request::LoadAndActivate(LoadRequest)`",
        "`hibana::substrate::mgmt::session::Request::Activate(SlotRequest)`",
        "`hibana::substrate::mgmt::session::Request::Revert(SlotRequest)`",
        "`hibana::substrate::mgmt::session::Request::Stats(SlotRequest)`",
        "`hibana::substrate::mgmt::TapBatch`",
        "`hibana::runtime::mgmt::TapBatch`",
        "`enter_controller`",
        "`enter_cluster`",
        "`drive_cluster`",
        ".drive_controller(controller)",
    ] {
        assert!(
            !readme.contains(forbidden),
            "README must not keep the deleted management helper surface: {forbidden}"
        );
    }

    for line in readme.lines() {
        let trimmed = line.trim_start();
        let is_role_or_message_synonym = trimmed.contains("= Role<") || trimmed.contains("= Msg<");
        let is_step_or_projection_shorthand = trimmed.contains("= StepCons<")
            || trimmed.contains("= SeqSteps<")
            || trimmed.contains("= LoopContinueSteps<")
            || trimmed.contains("= LoopBreakSteps<")
            || trimmed.contains("= LoopDecisionSteps<")
            || (trimmed.contains("= <") && trimmed.contains("as ProjectRole<"))
            || (trimmed.contains("= <") && trimmed.contains("as StepConcat<"));
        assert!(
            !(trimmed.starts_with("type ")
                && (is_role_or_message_synonym || is_step_or_projection_shorthand)),
            "README must not teach owner-hiding type aliases: {trimmed}"
        );
    }

    for forbidden in [
        "[[example]]",
        "name = \"tcp_tokio\"",
        "name = \"custom_binding\"",
        "name = \"transactional_session\"",
        "name = \"distributed_migration\"",
        "name = \"mgmt_epf_control\"",
        "test-utils = []",
        "hmac = ",
        "sha2 = ",
        "rand = ",
        "rand_chacha = ",
    ] {
        assert!(
            !cargo_toml.contains(forbidden),
            "crate manifest must not keep stale generic examples or their dead dependencies: {forbidden}"
        );
    }
}
