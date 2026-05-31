mod common;

use std::fs;
use std::mem::size_of_val;
use std::path::{Path, PathBuf};

use hibana::g;
use hibana::integration::program::{RoleProgram, project};
use hibana::integration::{
    SessionKit,
    runtime::{CounterClock, DefaultLabelUniverse},
};
use hibana::{Endpoint, RouteBranch};
use static_assertions::assert_not_impl_any;

type StaticTestKit =
    SessionKit<'static, common::TestTransport, DefaultLabelUniverse, CounterClock, 2>;

assert_not_impl_any!(StaticTestKit: Send, Sync);
assert_not_impl_any!(Endpoint<'static, 0>: Send, Sync);
assert_not_impl_any!(RouteBranch<'static, 'static, 0>: Send, Sync);

fn read(path: &str) -> String {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let full = root.join(path);
    fs::read_to_string(&full)
        .unwrap_or_else(|err| panic!("read {} failed: {}", full.display(), err))
}

fn read_dir_rs(path: &str) -> String {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path);
    let mut parts = fs::read_dir(&root)
        .unwrap_or_else(|err| panic!("read {} failed: {err}", root.display()))
        .map(|entry| {
            entry
                .unwrap_or_else(|err| panic!("read dir entry in {} failed: {err}", root.display()))
                .path()
        })
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("rs"))
        .collect::<Vec<_>>();
    parts.sort();
    let mut source = String::new();
    for part in parts {
        source.push_str(
            &fs::read_to_string(&part)
                .unwrap_or_else(|err| panic!("read {} failed: {err}", part.display())),
        );
    }
    source
}

fn integration_source() -> String {
    let mut source = read("src/integration.rs");
    source.push_str(&read_dir_rs("src/integration"));
    source
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

fn collect_source_files(dir: &Path, files: &mut Vec<PathBuf>) {
    for entry in
        fs::read_dir(dir).unwrap_or_else(|err| panic!("read {} failed: {err}", dir.display()))
    {
        let entry =
            entry.unwrap_or_else(|err| panic!("read dir entry in {} failed: {err}", dir.display()));
        let path = entry.path();
        if path.is_dir() {
            collect_source_files(&path, files);
        } else if matches!(
            path.extension().and_then(|ext| ext.to_str()),
            Some("rs" | "md")
        ) {
            files.push(path);
        }
    }
}

#[test]
fn projection_surface_still_builds() {
    let program = g::send::<0, 1, g::Msg<1, u8>, 0>();
    let _: RoleProgram<0> = project(&program);
}

#[test]
fn integration_facade_projects_before_enter() {
    let program = g::seq(
        g::send::<0, 1, g::Msg<1, u8>, 0>(),
        g::send::<1, 0, g::Msg<2, u8>, 0>(),
    );

    let _: RoleProgram<0> = project(&program);
    let _: RoleProgram<1> = project(&program);
}

#[test]
fn witness_sizes_stay_small() {
    let program = g::send::<0, 1, g::Msg<1, u8>, 0>();
    let role: RoleProgram<0> = project(&program);

    assert_eq!(size_of_val(&program), 0, "Program<Steps> must stay ZST");
    assert!(
        size_of_val(&role) <= 24,
        "RoleProgram<ROLE> must stay within the final witness budget"
    );
}

#[test]
fn session_kit_construction_is_in_place_only() {
    let integration_rs = integration_source();
    let allowlist = read(".github/allowlists/integration-public-api.txt");

    assert!(
        integration_rs.contains("pub struct SessionKitStorage")
            && integration_rs.contains("pub fn init(&mut self) -> &SessionKit")
            && !integration_rs.contains("pub fn init_in_place")
            && !integration_rs.contains("pub mod resident {")
            && !integration_rs.contains("pub fn init_resident_in_place")
            && !integration_rs.contains("pub unsafe fn init_in_place("),
        "SessionKit construction must expose one safe Pico-class storage owner"
    );
    assert!(
        !integration_rs.contains("pub fn new(clock:"),
        "SessionKit must not expose owned construction; use SessionKitStorage::uninit().init()"
    );
    assert!(
        allowlist.contains("pub struct SessionKitStorage")
            && allowlist.contains("pub fn init(&mut self) -> &SessionKit")
            && !allowlist.contains("pub fn init_in_place")
            && !allowlist.contains("init_resident_in_place"),
        "integration allowlist must list only the unified safe storage construction path"
    );
    assert!(
        !allowlist.contains("pub fn new(clock:"),
        "integration allowlist must not retain owned SessionKit construction"
    );
}

#[test]
fn clock_authority_is_config_only() {
    let integration_rs = compact_ws(&integration_source());
    let cluster_rs = read("src/control/cluster/core.rs");
    let allowlist = compact_ws(&read(".github/allowlists/integration-public-api.txt"));
    let readme = read("README.md");
    let crate_docs = read("src/lib.rs");

    assert!(
        integration_rs.contains("pub struct SessionKitStorage")
            && allowlist.contains("pub struct SessionKitStorage")
            && integration_rs.contains("pub fn init(&mut self) -> &SessionKit")
            && !integration_rs.contains("pub fn init_in_place")
            && !integration_rs.contains("pub mod resident {")
            && !allowlist.contains("pub mod resident {")
            && !integration_rs.contains("init_resident_in_place")
            && !allowlist.contains("init_resident_in_place")
            && !integration_rs.contains("pub unsafe fn init_in_place(")
            && !allowlist.contains("pub unsafe fn init_in_place("),
        "SessionKit construction must keep raw unsafe initialization private and expose one storage API"
    );
    assert!(
        !integration_rs.contains(
            "pub unsafe fn init_in_place( storage: &'cfg mut core::mem::MaybeUninit<Self>, clock:"
        ) && !allowlist.contains(
            "pub unsafe fn init_in_place( storage: &'cfg mut core::mem::MaybeUninit<Self>, clock:"
        ),
        "resident init_in_place must not accept a clock; Config owns rendezvous clock authority"
    );
    assert!(
        !cluster_rs.contains("clock: &'cfg C") && !cluster_rs.contains("self.clock.now32()"),
        "SessionCluster must not retain a separate clock authority"
    );
    assert!(
        readme.contains("let kit = kit_storage.init();")
            && readme
                .contains("let config = Config::from_resources((&mut tap_buf, &mut slab), clock);")
            && crate_docs.contains("let kit = kit_storage.init();")
            && crate_docs.contains("clock,"),
        "public docs must teach unified storage-owned SessionKit construction and Config-owned clock authority"
    );
}

#[test]
fn docs_and_tests_do_not_teach_session_kit_new() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut files = vec![root.join("README.md"), root.join("src/lib.rs")];
    collect_source_files(&root.join("tests"), &mut files);
    let forbidden = concat!("SessionKit::", "new");

    let mut offenders = Vec::new();
    for file in files {
        let source = fs::read_to_string(&file)
            .unwrap_or_else(|err| panic!("read {} failed: {err}", file.display()));
        if source.contains(forbidden) {
            offenders.push(file.display().to_string());
        }
    }

    assert!(
        offenders.is_empty(),
        "owned SessionKit construction must not be documented or used; use SessionKitStorage::uninit().init():\n{}",
        offenders.join("\n")
    );
}

#[test]
fn frame_label_has_single_integration_owner() {
    let integration_rs = integration_source();

    let binding_block = integration_rs
        .split("pub mod binding {")
        .nth(1)
        .and_then(|tail| tail.split("/// Resolver and decision-input surface").next())
        .expect("integration binding bucket must precede the policy bucket");
    assert!(
        !binding_block.contains("FrameLabel"),
        "FrameLabel must not be re-exported from integration::binding"
    );

    let transport_block = integration_rs
        .split("pub mod transport {")
        .nth(1)
        .expect("integration transport bucket must stay present");
    assert!(
        transport_block.contains("FrameLabel"),
        "FrameLabel's single integration owner must be integration::transport"
    );
}

#[test]
fn integration_eff_index_surface_is_segmented_not_flat() {
    let _: fn(hibana::integration::ids::EffIndex) -> u16 =
        hibana::integration::ids::EffIndex::segment;
    let _: fn(hibana::integration::ids::EffIndex) -> u16 =
        hibana::integration::ids::EffIndex::offset;

    let eff_rs = read("src/eff.rs");
    assert!(
        eff_rs.contains("pub const fn segment(self) -> u16")
            && eff_rs.contains("pub const fn offset(self) -> u16"),
        "EffIndex must expose segment and segment-local offset as the integration shape"
    );
    assert!(
        !eff_rs.contains("pub const fn as_usize")
            && !eff_rs.contains("pub const fn raw")
            && !eff_rs.contains("pub const ZERO")
            && !eff_rs.contains("pub const MAX")
            && eff_rs.contains("pub(crate) const fn dense_ordinal(self) -> usize"),
        "EffIndex must not expose public constructors, sentinels, flat ordinal, or raw conversion"
    );
}

#[test]
fn role_program_handle_is_resident_compiled_image_backed() {
    let role_program = {
        let mut source = read("src/global/role_program.rs");
        source.push_str(&read_dir_rs("src/global/role_program"));
        source
    };
    let g = read("src/g.rs");
    let role_program_struct = role_program
        .split("pub struct RoleProgram<const ROLE: u8> {")
        .nth(1)
        .and_then(|tail| tail.split("}").next())
        .expect("RoleProgram definition must stay present");
    let role_projection = g
        .split("struct RoleProjection<const ROLE: u8, Steps>")
        .nth(1)
        .expect("role projection boundary must stay on the public g vocabulary side");

    assert!(
        !role_program_struct.contains("summary:")
            && !role_program_struct.contains("stamp:")
            && !role_program_struct.contains("source:")
            && !role_program.contains("const fn summary(")
            && !role_program.contains("pub(crate) const fn summary("),
        "RoleProgram must not store or expose a CompiledProgramImage-backed projection handle"
    );
    assert!(
        role_program_struct.contains("image: ProjectionWitness")
            && role_program
                .contains("struct ProjectionWitness(&'static crate::global::compiled::images::CompiledRoleImage)")
            && role_program.contains(
                "const fn new(image: &'static crate::global::compiled::images::CompiledRoleImage)"
            )
            && role_projection
                .contains("const IMAGE: crate::global::compiled::images::CompiledRoleImage")
            && role_projection.contains("CompiledRoleImage::new(")
            && role_projection.contains("CompiledProgramRef::resident("),
        "RoleProgram must stay a compact resident CompiledRoleImage handle"
    );
}

#[test]
fn integration_root_exposes_only_core_buckets() {
    let integration_rs = integration_source();
    let root_prefix = integration_rs
        .split("pub mod ids {")
        .next()
        .expect("integration source must contain ids bucket");
    assert!(
        !root_prefix.contains("pub use crate::control::types::{Lane, RendezvousId, SessionId}")
            && !root_prefix.contains("use crate::control::types::{RendezvousId, SessionId}")
            && !root_prefix.contains("pub use crate::eff::EffIndex"),
        "integration root must not keep identifier aliases outside integration::ids"
    );
    assert!(
        integration_rs.contains("crate::integration::ids::RendezvousId")
            && integration_rs.contains("crate::integration::ids::SessionId"),
        "SessionKit signatures must point callers at integration::ids"
    );

    for required in [
        "pub mod runtime {",
        "pub mod ids {",
        "pub mod binding {",
        "EndpointSlot",
        "BindingError",
        "Channel",
        "IngressEvidence",
        "pub mod policy {",
        "ResolverRef",
        "pub mod cap {",
        "WireControlEffect",
        "pub mod wire {",
        "pub mod transport {",
        "pub use crate::observe::core::TapEvent;",
        "pub use crate::eff::EffIndex;",
        "pub use crate::global::program::Projectable;",
        "Transport,",
        "WirePayload",
    ] {
        assert!(
            integration_rs.contains(required),
            "integration surface must keep the core bucket: {required}"
        );
    }

    let policy_root = integration_rs
        .split("pub mod policy {")
        .nth(1)
        .and_then(|tail| tail.split("/// Canonical capability-token surface").next())
        .expect("integration policy bucket must be followed by the cap bucket");
    for required in ["ResolverRef"] {
        assert!(
            policy_root.contains(required),
            "integration::policy must own the resolver surface: {required}"
        );
    }
    for forbidden in [
        "ContextId",
        "ContextValue",
        "PolicyInput",
        "PolicySignals,",
        "PolicySlot",
        "ResolverContext",
        "pub mod core",
        "pub mod replay {",
        "PolicyAttrs",
    ] {
        assert!(
            !policy_root.contains(forbidden),
            "policy root must not expose replay metadata or context internals: {forbidden}"
        );
    }
    assert!(
        !policy_root.contains("pub mod advanced {"),
        "integration::policy must not keep an advanced compatibility bucket"
    );

    let binding_root = integration_rs
        .split("pub mod binding {")
        .nth(1)
        .and_then(|tail| tail.split("/// Resolver and decision-input surface").next())
        .expect("integration binding bucket must precede policy");
    for required in ["BindingError", "EndpointSlot", "Channel", "IngressEvidence"] {
        assert!(
            binding_root.contains(required),
            "integration::binding root must contain every type needed to implement EndpointSlot: {required}"
        );
    }
    for forbidden in [
        "pub mod advanced {",
        "ChannelDirection",
        "ChannelKey",
        "ChannelStore",
    ] {
        assert!(
            !binding_root.contains(forbidden),
            "integration::binding must not keep a secondary advanced bucket or deleted channel surface: {forbidden}"
        );
    }

    for forbidden in [
        "pub mod mgmt {",
        "pub mod epf {",
        "crate::runtime::mgmt",
        "crate::epf",
        "WireDecode",
        "LocalDirection",
        "SendMeta",
        "TransportAlgorithm",
        "TransportMetricsTapPayload",
        "TransportAlgorithm, TransportError",
        "pub mod replay {",
        "PolicyAttrs",
        "pub mod advanced {",
        "advanced::policy",
        "pub mod inspect {",
        "ProjectionMetadataVisitor",
        "ProjectionProgramFacts",
    ] {
        assert!(
            !integration_rs.contains(forbidden),
            "integration surface must not keep deleted in-crate mgmt/epf owners: {forbidden}"
        );
    }
    assert!(
        integration_rs.contains("Transport,")
            && !integration_rs.contains("TransportEvent")
            && !integration_rs.contains("TransportEventKind")
            && !integration_rs.contains("TransportMetrics"),
        "transport surface must stay protocol-neutral I/O without events, metrics, or extension metadata"
    );
}

#[test]
fn runtime_and_lib_drop_incrate_mgmt_and_epf_modules() {
    let runtime_rs = read("src/runtime.rs");
    let lib_rs = read("src/lib.rs");

    for forbidden in ["mod mgmt;", "pub(crate) mod mgmt;"] {
        assert!(
            !runtime_rs.contains(forbidden),
            "runtime root must not wire the deleted in-crate mgmt owner: {forbidden}"
        );
    }
    assert!(
        !lib_rs.contains("mod epf;"),
        "lib root must not wire the deleted in-crate epf owner"
    );
}

#[test]
fn integration_allowlist_tracks_core_boundary() {
    let allowlist = compact_ws(&read(".github/allowlists/integration-public-api.txt"));

    for required in [
        "pub use crate::observe::core::TapEvent;",
        "RING_EVENTS",
        "pub struct SessionKitStorage",
        "Projectable",
        "BindingError",
        "WireControlEffect",
        "WirePayload",
    ] {
        assert!(
            allowlist.contains(required),
            "integration allowlist must track the surviving core surface: {required}"
        );
    }

    for forbidden in [
        "pub mod mgmt {",
        "pub mod epf {",
        "crate::runtime::mgmt",
        "crate::epf",
        "WireDecode",
        "LocalDirection",
        "SendMeta",
        "TransportAlgorithm",
        "TransportMetricsTapPayload",
        "TransportAlgorithm, TransportError",
        "pub fn new(clock:",
        "ProjectionMessageSpec",
        "ProjectionTypeFingerprint",
        "pub mod replay {",
        "PolicyAttrs",
        "pub mod advanced {",
        "advanced::policy",
        "pub mod inspect {",
        "ProjectionMetadataVisitor",
        "ProjectionProgramFacts",
    ] {
        assert!(
            !allowlist.contains(forbidden),
            "integration allowlist must not keep deleted or std/test-only buckets: {forbidden}"
        );
    }
    assert!(
        allowlist.contains("Transport, TransportError")
            && !allowlist.contains("TransportEvent")
            && !allowlist.contains("TransportEventKind"),
        "integration allowlist must keep transport I/O only"
    );
}

#[test]
fn crate_package_artifact_is_a_first_class_gate() {
    let cargo = read("Cargo.toml");
    let package_gate = read(".github/scripts/check_package_artifact.sh");
    let maintainability_gate = read(".github/scripts/check_maintainability_budgets.sh");
    let final_gate = read(".github/scripts/run_final_form_gates.sh");

    assert!(
        cargo.contains("\"/src/**\""),
        "crate package must include production source through the source tree"
    );
    assert!(
        cargo.contains("\"!/src/test_support/**\"")
            && cargo.contains("\"!/src/endpoint/kernel/test_support/**\"")
            && cargo.contains("\"!/src/**/tests.rs\"")
            && cargo.contains("\"!/src/**/tests/**\"")
            && cargo.contains("\"!/src/**/*_tests.rs\""),
        "crate package must exclude source-tree test fixtures from the production package"
    );
    assert!(
        !cargo.contains("autotests")
            && !cargo.contains("[[test]]")
            && !cargo.contains("\"/tests/**\"")
            && !cargo.contains("\"/tests/support/**\""),
        "repo integration tests must remain Cargo-auto-discovered locally without shipping in the production artifact"
    );
    for required in [
        "src must not depend on tests/support fixtures",
        "source-tree test fixtures must not ship in the production crate package",
        "repo integration tests must not ship in the production crate package",
        "SOURCE_TEST_FIXTURE_PATTERN",
        "^src/.*/tests/",
        "'^tests/'",
        "run_package_clean \"cargo package --list\"",
        "run_package_allowing_omitted_repo_tests \"cargo package --no-verify\"",
        "package lib check --features std",
        "package lib test build --features std",
        "package test build --features std",
        "package lib check --no-default-features",
        "package lib test build --no-default-features",
        "package docs --no-default-features",
        "RUSTFLAGS=\"-Dwarnings\"",
        "RUSTDOCFLAGS=\"-Dwarnings\"",
    ] {
        assert!(
            package_gate.contains(required),
            "package artifact gate must verify package contents after checkout gates: {required}"
        );
    }
    assert!(
        maintainability_gate
            .contains("integration tests must not path-import src/test_support fixtures"),
        "maintainability gate must keep integration fixtures from reaching into src/test_support"
    );
    assert!(
        final_gate.contains("bash ./.github/scripts/check_package_artifact.sh"),
        "final gate must run package artifact verification before release"
    );
}
