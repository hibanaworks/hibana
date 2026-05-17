mod common;

use std::fs;
use std::mem::size_of_val;
use std::path::PathBuf;

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
fn projection_surface_still_builds() {
    let program = g::send::<g::Role<0>, g::Role<1>, g::Msg<1, u8>, 0>();
    let _: RoleProgram<0> = project(&program);
}

#[test]
fn integration_facade_projects_before_enter() {
    let program = g::seq(
        g::send::<g::Role<0>, g::Role<1>, g::Msg<1, u8>, 0>(),
        g::send::<g::Role<1>, g::Role<0>, g::Msg<2, u8>, 0>(),
    );

    let _: RoleProgram<0> = project(&program);
    let _: RoleProgram<1> = project(&program);
}

#[test]
fn witness_sizes_stay_small() {
    let program = g::send::<g::Role<0>, g::Role<1>, g::Msg<1, u8>, 0>();
    let role: RoleProgram<0> = project(&program);

    assert_eq!(size_of_val(&program), 0, "Program<Steps> must stay ZST");
    assert!(
        size_of_val(&role) <= 24,
        "RoleProgram<ROLE> must stay within the final witness budget"
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
    let role_program = read("src/global/role_program.rs");
    let role_program_struct = role_program
        .split("pub struct RoleProgram<const ROLE: u8> {")
        .nth(1)
        .and_then(|tail| tail.split("}").next())
        .expect("RoleProgram definition must stay present");
    let validated_role_image = role_program
        .split("struct ValidatedRoleImage<Steps, const ROLE: u8>")
        .nth(1)
        .expect("validated role image definition must stay present");

    assert!(
        !role_program_struct.contains("summary:")
            && !role_program_struct.contains("stamp:")
            && !role_program_struct.contains("source:")
            && !role_program.contains("const fn summary(")
            && !role_program.contains("pub(crate) const fn summary("),
        "RoleProgram must not store or expose a CompiledProgramImage-backed projection handle"
    );
    assert!(
        role_program_struct
            .contains("image: &'static crate::global::compiled::images::CompiledRoleImage")
            && validated_role_image.contains("const COMPILED_IMAGE")
            && validated_role_image.contains("CompiledRoleImage::new(")
            && validated_role_image.contains("CompiledProgramRef::resident("),
        "RoleProgram must stay a compact resident CompiledRoleImage handle"
    );
}

#[test]
fn integration_root_exposes_only_core_buckets() {
    let integration_rs = read("src/integration.rs");
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
        "pub mod tap {",
        "pub mod binding {",
        "pub use crate::binding::{BindingSlot, NoBinding};",
        "pub mod advanced {",
        "ChannelStore",
        "IngressEvidence",
        "pub mod policy {",
        "pub use crate::transport::context::PolicySignalsProvider;",
        "pub mod signals {",
        "ContextId, ContextValue, PolicyAttrs, PolicySignals",
        "pub mod core {",
        "pub mod cap {",
        "pub mod wire {",
        "pub mod transport {",
        "pub use crate::observe::core::TapEvent;",
        "pub use crate::policy_runtime::PolicySlot;",
        "pub use crate::eff::EffIndex;",
        "TransportMetrics",
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
    for required in ["PolicySignalsProvider", "pub mod signals {", "pub mod core"] {
        assert!(
            policy_root.contains(required),
            "integration::policy must own the resolver/provider surface and signals bucket: {required}"
        );
    }
    let policy_root_before_signals = policy_root
        .split("pub mod signals {")
        .next()
        .expect("policy root must contain the signals bucket");
    for forbidden in [
        "ContextId",
        "ContextValue",
        "PolicyAttrs",
        "PolicySignals,",
        "PolicySlot",
    ] {
        assert!(
            !policy_root_before_signals.contains(forbidden),
            "policy root must not expose signal metadata directly: {forbidden}"
        );
    }
    assert!(
        !policy_root.contains("pub mod advanced {"),
        "integration::policy must not keep an advanced compatibility bucket"
    );

    let binding_root = integration_rs
        .split("pub mod binding {")
        .nth(1)
        .and_then(|tail| tail.split("pub mod advanced {").next())
        .expect("integration binding bucket must keep an advanced detail bucket");
    for forbidden in [
        "Channel",
        "ChannelDirection",
        "ChannelKey",
        "ChannelStore",
        "IngressEvidence",
        "TransportOpsError",
    ] {
        assert!(
            !binding_root.contains(forbidden),
            "integration::binding root must stay on BindingSlot + NoBinding; detail belongs under binding::advanced: {forbidden}"
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
        "TransportError, TransportEvent",
    ] {
        assert!(
            !integration_rs.contains(forbidden),
            "integration surface must not keep deleted in-crate mgmt/epf owners: {forbidden}"
        );
    }
    assert!(
        integration_rs.contains("TransportEvent, TransportEventKind, TransportMetrics"),
        "transport event-kind and metrics detail must live in the advanced bucket"
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
        "pub mod tap {",
        "pub use crate::observe::core::TapEvent;",
        "pub mod advanced {",
        "pub mod signals {",
        "pub use crate::policy_runtime::PolicySlot;",
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
        "TransportError, TransportEvent",
    ] {
        assert!(
            !allowlist.contains(forbidden),
            "integration allowlist must not keep deleted mgmt/epf buckets: {forbidden}"
        );
    }
    assert!(
        allowlist.contains("TransportEvent, TransportEventKind, TransportMetrics"),
        "integration allowlist must keep transport event-kind detail in advanced"
    );
}
