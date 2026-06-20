mod common;

use std::fs;
use std::mem::size_of_val;
use std::path::{Path, PathBuf};

use hibana::g;
use hibana::runtime::SessionKit;
use hibana::runtime::program::{RoleProgram, project};
use hibana::runtime::resolver::{DecisionArm, ResolverError, ResolverRef};
use hibana::{Endpoint, RouteBranch};
use static_assertions::assert_not_impl_any;

type StaticTestKit = SessionKit<'static, common::TestTransport>;

assert_not_impl_any!(StaticTestKit: Send, Sync);
assert_not_impl_any!(Endpoint<'static, 0>: Send, Sync);
assert_not_impl_any!(RouteBranch<'static, 'static, 0>: Send, Sync);

#[test]
fn test_transport_support_constructs_explicitly() {
    let transport = common::TestTransport::new();
    assert!(transport.queue_is_empty());
}

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

fn runtime_source() -> String {
    let mut source = read("src/runtime.rs");
    source.push_str(&read_dir_rs("src/runtime"));
    source
}

fn runtime_public_surface_source() -> String {
    let mut source = read("src/runtime.rs");
    source.push_str(&read("src/runtime/buckets.rs"));
    source.push_str(&read("src/runtime/session_kit.rs"));
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
    let program = g::send::<0, 1, g::Msg<1, u8>>();
    let _: RoleProgram<0> = project(&program);
}

#[test]
fn runtime_facade_projects_before_enter() {
    let program = g::seq(
        g::send::<0, 1, g::Msg<1, u8>>(),
        g::send::<1, 0, g::Msg<2, u8>>(),
    );

    let _: RoleProgram<0> = project(&program);
    let _: RoleProgram<1> = project(&program);
}

const EXTERNAL_RESOLVER_ID: u16 = 91;

struct LocalResolver {
    available: bool,
}

struct ExternalResolver<'a> {
    loaded: bool,
    local_resolver: ResolverRef<'a, EXTERNAL_RESOLVER_ID>,
}

fn local_resolver_decision(resolver: &LocalResolver) -> Result<DecisionArm, ResolverError> {
    if resolver.available {
        Ok(DecisionArm::Left)
    } else {
        Err(ResolverError::reject())
    }
}

fn external_resolver_decision(
    resolver: &ExternalResolver<'_>,
) -> Result<DecisionArm, ResolverError> {
    if resolver.loaded {
        Ok(DecisionArm::Right)
    } else {
        resolver.local_resolver.decide()
    }
}

#[test]
fn resolver_state_can_host_external_resolver_owner() {
    let local = LocalResolver { available: true };
    let local_resolver =
        ResolverRef::<EXTERNAL_RESOLVER_ID>::decision_state(&local, local_resolver_decision);
    let unloaded = ExternalResolver {
        loaded: false,
        local_resolver,
    };
    let resolver =
        ResolverRef::<EXTERNAL_RESOLVER_ID>::decision_state(&unloaded, external_resolver_decision);
    assert_eq!(resolver.decide(), Ok(DecisionArm::Left));

    let loaded = ExternalResolver {
        loaded: true,
        local_resolver,
    };
    let resolver =
        ResolverRef::<EXTERNAL_RESOLVER_ID>::decision_state(&loaded, external_resolver_decision);
    assert_eq!(resolver.decide(), Ok(DecisionArm::Right));
}

#[test]
fn witness_sizes_stay_small() {
    let program = g::send::<0, 1, g::Msg<1, u8>>();
    let role: RoleProgram<0> = project(&program);

    assert_eq!(size_of_val(&program), 0, "Program<Steps> must stay ZST");
    assert!(
        size_of_val(&role) <= 24,
        "RoleProgram<ROLE> must stay within the final witness budget"
    );
}

#[test]
fn session_kit_construction_is_in_place_only() {
    let runtime_rs = runtime_source();
    let allowlist = read(".github/allowlists/runtime-public-api.txt");

    assert!(
        runtime_rs.contains("pub struct SessionKitStorage")
            && runtime_rs.contains("pub fn init(&mut self) -> &SessionKit")
            && !runtime_rs.contains("pub fn init_in_place")
            && !runtime_rs.contains("pub mod resident {")
            && !runtime_rs.contains("pub fn init_resident_in_place")
            && !runtime_rs.contains("pub unsafe fn init_in_place("),
        "SessionKit construction must expose one safe Pico-class storage owner"
    );
    assert!(
        !runtime_rs.contains("pub fn new(clock:"),
        "SessionKit must not expose owned construction; use SessionKitStorage::uninit().init()"
    );
    assert!(
        allowlist.contains("pub struct SessionKitStorage")
            && allowlist.contains("pub fn init(&mut self) -> &SessionKit")
            && !allowlist.contains("pub fn init_in_place")
            && !allowlist.contains("init_resident_in_place"),
        "runtime allowlist must list only the unified safe storage construction path"
    );
    assert!(
        !allowlist.contains("pub fn new(clock:"),
        "runtime allowlist must not retain owned SessionKit construction"
    );
}

#[test]
fn single_slab_config_is_runtime_resource_authority() {
    let runtime_rs = compact_ws(&runtime_source());
    let cluster_rs = read("src/session/cluster/core.rs");
    let allowlist = compact_ws(&read(".github/allowlists/runtime-public-api.txt"));
    let readme = read("README.md");
    let crate_docs = read("src/lib.rs");

    assert!(
        runtime_rs.contains("pub struct SessionKitStorage")
            && allowlist.contains("pub struct SessionKitStorage")
            && runtime_rs.contains("pub fn init(&mut self) -> &SessionKit")
            && !runtime_rs.contains("pub fn init_in_place")
            && !runtime_rs.contains("pub mod resident {")
            && !allowlist.contains("pub mod resident {")
            && !runtime_rs.contains("init_resident_in_place")
            && !allowlist.contains("init_resident_in_place")
            && !runtime_rs.contains("pub unsafe fn init_in_place(")
            && !allowlist.contains("pub unsafe fn init_in_place("),
        "SessionKit construction must keep raw unsafe initialization private and expose one storage API"
    );
    assert!(
        !runtime_rs.contains(
            "pub unsafe fn init_in_place( storage: &'cfg mut core::mem::MaybeUninit<Self>, clock:"
        ) && !allowlist.contains(
            "pub unsafe fn init_in_place( storage: &'cfg mut core::mem::MaybeUninit<Self>, clock:"
        ),
        "resident init_in_place must not accept a clock; rendezvous owns the single runtime slab authority"
    );
    assert!(
        !cluster_rs.contains("clock: &'cfg C") && !cluster_rs.contains("self.clock.now32()"),
        "SessionCluster must not retain a separate clock authority"
    );
    assert!(
        readme.contains("let kit = kit_storage.init();")
            && readme.contains("let rv = kit.rendezvous(&mut slab, transport)?;")
            && crate_docs.contains("let kit = kit_storage.init();")
            && crate_docs.contains("let rv = kit.rendezvous(&mut slab, transport)?;")
            && !readme.contains("CounterClock")
            && !readme.contains("tap_buf")
            && !readme.contains("Config::from_resources")
            && !crate_docs.contains("runtime::Config")
            && !crate_docs.contains("CounterClock")
            && !crate_docs.contains("tap_buf"),
        "public docs must teach unified storage-owned SessionKit construction and direct single-slab rendezvous authority"
    );
}

#[test]
fn docs_and_tests_do_not_teach_session_kit_new() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut files = vec![root.join("README.md"), root.join("src/lib.rs")];
    collect_source_files(&root.join("tests"), &mut files);
    let forbidden = "SessionKit::new";

    let mut offenders = Vec::new();
    for file in files {
        if file
            .strip_prefix(&root)
            .map(|relative| {
                relative == Path::new("tests/runtime_surface.rs")
                    || relative == Path::new("tests/docs_surface.rs")
            })
            .unwrap_or(false)
        {
            continue;
        }
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
fn frame_label_has_single_runtime_owner() {
    let runtime_rs = runtime_source();

    assert!(
        !runtime_rs.contains("pub mod binding {"),
        "FrameLabel must not be hidden behind a public binding bucket"
    );

    let transport_block = runtime_rs
        .split("pub mod transport {")
        .nth(1)
        .expect("runtime transport bucket must stay present");
    assert!(
        transport_block.contains("FrameLabel"),
        "FrameLabel's single runtime owner must be runtime::transport"
    );
}

#[test]
fn eff_index_stays_internal_descriptor_id() {
    let runtime_rs = runtime_source();
    let eff_rs = read("src/eff.rs");
    assert!(
        !runtime_rs.contains("EffIndex")
            && eff_rs.contains("pub(crate) struct EffIndex")
            && eff_rs.contains("pub(crate) const fn segment(self) -> u16")
            && eff_rs.contains("pub(crate) const fn offset(self) -> u16"),
        "EffIndex must remain an internal segmented descriptor id, not public runtime authority"
    );
    assert!(
        !eff_rs.contains("pub const fn as_usize")
            && !eff_rs.contains("pub const fn raw")
            && !eff_rs.contains("pub const ZERO")
            && !eff_rs.contains("pub const MAX")
            && eff_rs.contains("pub(crate) const fn dense_ordinal(self) -> usize"),
        "EffIndex must not expose public constructors, absence codes, flat ordinal, or raw conversion"
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
    let role_blob = read("src/global/role_program/image_impl/blob_image.rs");
    let role_projection_source = read("src/g/role_projection.rs");
    let role_program_struct = role_program
        .split("pub struct RoleProgram<const ROLE: u8> {")
        .nth(1)
        .and_then(|tail| tail.split("}").next())
        .expect("RoleProgram definition must stay present");
    let role_projection = role_projection_source
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
        role_program_struct.contains("image: &'static crate::global::role_program::RoleImageRef")
            && !role_program.contains("struct ProjectionWitness")
            && g.contains("mod role_projection;")
            && role_projection
                .contains("const IMAGE_REF: crate::global::role_program::RoleImageRef")
            && role_projection.contains("ProgramImageBytes")
            && role_projection.contains("ProgramProjection::<Steps>::PROGRAM_REF")
            && role_projection.contains("RoleImageBuild<N>")
            && role_projection.contains("Self::image_ref(build)")
            && role_blob.contains("self.bytes.image_ref("),
        "RoleProgram must stay a direct compact resident RoleImageRef handle"
    );
}

#[test]
fn runtime_root_exposes_only_core_buckets() {
    let runtime_rs = runtime_source();
    let root_prefix = runtime_rs
        .split("pub mod ids {")
        .next()
        .expect("runtime source must contain ids bucket");
    assert!(
        !root_prefix.contains("pub use crate::session::types::{Lane, RendezvousId, SessionId}")
            && !root_prefix.contains("use crate::session::types::{RendezvousId, SessionId}")
            && !root_prefix.contains("pub use crate::eff::EffIndex"),
        "runtime root must not keep identifier aliases outside runtime::ids"
    );
    assert!(
        runtime_rs.contains("Result<RendezvousKit<'_, 'cfg, T>, AttachError>")
            && runtime_rs.contains("pub fn rendezvous(")
            && runtime_rs.contains("pub struct RendezvousKit")
            && runtime_rs.contains("pub fn enter<const ROLE: u8>")
            && runtime_rs.contains("pub fn set_resolver<const ROLE: u8, const RESOLVER: u16>")
            && !runtime_rs.contains("pub fn add_rendezvous(")
            && !runtime_rs.contains("pub fn add_rendezvous( &self")
            && !runtime_rs.contains("crate::runtime::ids::RendezvousId")
            && runtime_rs.contains("crate::runtime::ids::SessionId")
            && !runtime_rs.contains("HAS_SESSION")
            && !runtime_rs.contains("pub struct SessionKit<'cfg, T, const MAX_RV")
            && !runtime_rs.contains("pub struct SessionKitStorage<'cfg, T, const MAX_RV")
            && !runtime_rs.contains("RendezvousKit<'_, 'cfg, T, false")
            && !runtime_rs.contains("RoleKit<'kit, 'cfg, 'prog, const ROLE: u8"),
        "SessionKit must expose direct rendezvous operations without caller-selected capacity, raw RendezvousId attach authority, fluent witnesses, or bool typestate"
    );

    for required in [
        "RendezvousKit",
        "SessionKitStorage",
        "pub mod ids {",
        "pub mod tap {",
        "pub use crate::observe::core::{Evidence, TapEvent, TapPort};",
        "pub mod resolver {",
        "ResolverRef",
        "pub mod wire {",
        "pub mod transport {",
        "pub use crate::global::program::Projectable;",
        "Transport,",
        "WirePayload",
    ] {
        assert!(
            runtime_rs.contains(required),
            "runtime surface must keep the core bucket: {required}"
        );
    }
    assert!(
        !runtime_rs.contains("RuntimeStorage"),
        "runtime resources must be owned by rendezvous without a public storage envelope"
    );
    assert!(
        !runtime_rs.contains("pub use crate::runtime_core::") || !runtime_rs.contains("Config"),
        "runtime surface must not re-export a public Config wrapper"
    );
    let runtime_public = runtime_public_surface_source();
    for forbidden in ["Clock", "CounterClock", "RING_EVENTS", "TAP_EVENTS"] {
        assert!(
            !runtime_public.contains(forbidden),
            "runtime surface must not expose public clock or tap-buffer resources: {forbidden}"
        );
    }
    assert!(
        !runtime_public
            .lines()
            .any(|line| line.trim() == "pub use crate::observe::core::TapEvent;"),
        "TapEvent diagnostics must stay under runtime::tap, not the runtime root"
    );
    assert!(
        !runtime_rs.contains("pub mod binding {") && !runtime_rs.contains("pub fn ingress("),
        "ingress binding must not remain a public runtime bucket or attach verb"
    );

    let resolver_root = runtime_rs
        .split("pub mod resolver {")
        .nth(1)
        .and_then(|tail| tail.split("/// Wire payload codec surface.").next())
        .expect("runtime resolver bucket must be followed by the wire bucket");
    {
        let required = "ResolverRef";
        assert!(
            resolver_root.contains(required),
            "runtime::resolver must own the resolver surface: {required}"
        );
    }
    for forbidden in [
        "ContextId",
        "ContextValue",
        "ResolverInput",
        "ResolverSignals,",
        "ResolverSlot",
        "ResolverContext",
        "pub mod core",
        "pub mod replay {",
        "ResolverAttrs",
    ] {
        assert!(
            !resolver_root.contains(forbidden),
            "resolver root must not expose replay metadata or context internals: {forbidden}"
        );
    }
    assert!(
        !resolver_root.contains("pub mod advanced {"),
        "runtime::resolver must not keep an extra resolver bucket"
    );

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
        "EffIndex",
        "IngressEvidence",
        "pub mod replay {",
        "ResolverAttrs",
        "pub mod advanced {",
        "advanced::resolver",
        "pub mod inspect {",
        "ProjectionMetadataVisitor",
        "ProjectionProgramFacts",
    ] {
        assert!(
            !runtime_rs.contains(forbidden),
            "runtime surface must not keep forbidden in-crate mgmt/epf owners: {forbidden}"
        );
    }
    assert!(
        runtime_rs.contains("Transport,")
            && !runtime_rs.contains("TransportEvent")
            && !runtime_rs.contains("TransportEventKind")
            && !runtime_rs.contains("TransportMetrics"),
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
            "runtime root must not wire the forbidden in-crate mgmt owner: {forbidden}"
        );
    }
    assert!(
        !lib_rs.contains("mod epf;"),
        "lib root must not wire the forbidden in-crate epf owner"
    );
}

#[test]
fn runtime_allowlist_tracks_core_boundary() {
    let allowlist = compact_ws(&read(".github/allowlists/runtime-public-api.txt"));

    for required in [
        "pub mod tap {",
        "pub use crate::observe::core::{Evidence, TapEvent, TapPort};",
        "pub struct SessionKit<'cfg, T>",
        "pub struct SessionKitStorage<'cfg, T>",
        "pub struct RendezvousKit<'kit, 'cfg, T>",
        "RendezvousKit::enter",
        "RendezvousKit::set_resolver",
        "Result<RendezvousKit<'_, 'cfg, T>, AttachError>",
        "Projectable",
        "WirePayload",
    ] {
        assert!(
            allowlist.contains(required),
            "runtime allowlist must track the surviving core surface: {required}"
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
        "pub fn ingress(",
        "pub mod binding {",
        "pub mod cap {",
        "IngressSlot",
        "IngressError",
        "pub use crate::session::types::{Lane, SessionId};",
        "pub use crate::session::types::Lane",
        "runtime::ids::{EffIndex, Lane, SessionId}",
        "EffIndex",
        "IngressEvidence",
        "pub mod replay {",
        "ResolverAttrs",
        "pub mod advanced {",
        "advanced::resolver",
        "pub mod inspect {",
        "ProjectionMetadataVisitor",
        "ProjectionProgramFacts",
        "Clock",
        "CounterClock",
        "RING_EVENTS",
        "TAP_EVENTS",
        "HAS_SESSION",
        "const MAX_RV",
        "SessionKitStorage::<T, N>",
        "caller-owned local rendezvous budget",
        "RendezvousKit<'_, 'cfg, T, false",
        "SessionRendezvousKit",
        "SessionRoleKit",
        "RoleKit<'kit, 'cfg, 'prog, const ROLE: u8, T, false",
        "pub use crate::observe::core::TapEvent;",
    ] {
        assert!(
            !allowlist.contains(forbidden),
            "runtime allowlist must not keep forbidden or std/test-only buckets: {forbidden}"
        );
    }
    assert!(
        allowlist.contains("Transport, TransportError")
            && !allowlist.contains("TransportEvent")
            && !allowlist.contains("TransportEventKind"),
        "runtime allowlist must keep transport I/O only"
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
        "crate package must exclude source-tree test support from the production package"
    );
    assert!(
        !cargo.contains("autotests")
            && !cargo.contains("[[test]]")
            && cargo.contains("\"/tests/**\""),
        "package tests must remain Cargo-auto-discovered while repo-only gates stay outside the crate package"
    );
    for excluded in [
        "\"!/tests/docs_surface.rs\"",
        "\"!/tests/local_only_hygiene.rs\"",
        "\"!/tests/no_default_rodata.rs\"",
        "\"!/tests/public_surface_guards.rs\"",
        "\"!/tests/root_surface.rs\"",
        "\"!/tests/runtime_surface.rs\"",
        "\"!/tests/semantic_surface.rs\"",
        "\"!/tests/semantic_surface/**\"",
        "\"!/tests/transport_resolver_signal_surface.rs\"",
    ] {
        assert!(
            cargo.contains(excluded),
            "crate package must exclude repo-only gate source: {excluded}"
        );
    }
    for forbidden in [
        ".github/",
        ".github/allowlists/",
        ".github/measurement_snapshots/",
        ".github/maintainability/",
        "tests/semantic_surface.rs",
        "tests/semantic_surface/",
        "tests/public_surface_guards.rs",
        "tests/runtime_surface.rs",
        "tests/root_surface.rs",
        "tests/docs_surface.rs",
    ] {
        assert!(
            package_gate.contains(forbidden),
            "package artifact gate must reject repo-only package contents: {forbidden}"
        );
    }
    for required in [
        "src must not depend on tests/support",
        "source-tree test support must not ship in the production crate package",
        "SOURCE_TEST_SUPPORT_PATTERN",
        "^src/.*/tests/",
        "run_package_clean \"cargo package --list\"",
        "run_package_with_repo_test_exclusions \"cargo package --no-verify\"",
        "package lib check",
        "package lib test",
        "packaged tests must include their module tree",
        "package UI harness",
        "--test ui",
        "package behavior test",
        "--test lane_lifecycle_tap",
        "package lib check --no-default-features",
        "package lib test --no-default-features",
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
        maintainability_gate.contains("repository tests must not path-import src/test_support"),
        "maintainability gate must keep runtime test support from reaching into src/test_support"
    );
    assert!(
        final_gate.contains("bash ./.github/scripts/check_package_artifact.sh"),
        "final gate must run package artifact verification before release"
    );
    assert!(
        final_gate.contains("bash ./.github/scripts/check_hibana_public_api.sh --surface-only"),
        "final gate must reuse the all-test pass instead of rerunning public surface tests"
    );
    assert!(
        final_gate.contains("bash ./.github/scripts/check_boundary_contracts.sh --local-only"),
        "final gate must not rerun boundary sub-gates through the aggregate script"
    );
    for duplicate in [
        "bash ./.github/scripts/check_public_surface_budget.sh",
        "bash ./.github/scripts/check_surface_hygiene.sh",
    ] {
        assert!(
            !final_gate.contains(duplicate),
            "final gate must not duplicate API checks already owned by check_hibana_public_api.sh: {duplicate}"
        );
    }
}
