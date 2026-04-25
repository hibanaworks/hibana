mod common;

use std::fs;
use std::mem::size_of_val;
use std::path::PathBuf;

use hibana::g;
use hibana::substrate::program::{RoleProgram, project};
use hibana::substrate::{
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
fn role_program_handle_is_not_summary_backed() {
    let role_program = read("src/global/role_program.rs");
    let role_image = role_program
        .split("struct RoleImage {")
        .nth(1)
        .and_then(|tail| tail.split("}").next())
        .expect("RoleImage definition must stay present");

    assert!(
        !role_image.contains("summary:")
            && !role_program.contains("const fn summary(self) -> &'static LoweringSummary")
            && !role_program
                .contains("pub(crate) const fn summary(&self) -> &'static LoweringSummary"),
        "RoleProgram must not store or expose a LoweringSummary-backed projection handle"
    );
    assert!(
        role_image.contains("stamp: ProgramStamp")
            && role_image.contains("facts: RoleFacts")
            && role_image.contains("source: RoleImageSource"),
        "RoleProgram must stay a compact verified descriptor handle plus sealed materialization source"
    );
}

#[test]
fn substrate_root_exposes_only_core_buckets() {
    let substrate_rs = read("src/substrate.rs");
    let root_prefix = substrate_rs
        .split("pub mod ids {")
        .next()
        .expect("substrate source must contain ids bucket");
    assert!(
        !root_prefix.contains("pub use crate::control::types::{Lane, RendezvousId, SessionId}")
            && !root_prefix.contains("use crate::control::types::{RendezvousId, SessionId}")
            && !root_prefix.contains("pub use crate::eff::EffIndex"),
        "substrate root must not keep identifier aliases outside substrate::ids"
    );
    assert!(
        substrate_rs.contains("crate::substrate::ids::RendezvousId")
            && substrate_rs.contains("crate::substrate::ids::SessionId"),
        "SessionKit signatures must point callers at substrate::ids"
    );

    for required in [
        "pub mod runtime {",
        "pub mod ids {",
        "pub mod tap {",
        "pub mod binding {",
        "pub use crate::binding::{BindingSlot, NoBinding};",
        "pub mod advanced {",
        "ChannelStore",
        "IncomingClassification",
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
            substrate_rs.contains(required),
            "substrate surface must keep the core bucket: {required}"
        );
    }

    let policy_root = substrate_rs
        .split("pub mod policy {")
        .nth(1)
        .and_then(|tail| tail.split("/// Canonical capability-token surface").next())
        .expect("substrate policy bucket must be followed by the cap bucket");
    for required in ["PolicySignalsProvider", "pub mod signals {", "pub mod core"] {
        assert!(
            policy_root.contains(required),
            "substrate::policy must own the resolver/provider surface and signals bucket: {required}"
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
        "substrate::policy must not keep an advanced compatibility bucket"
    );

    let binding_root = substrate_rs
        .split("pub mod binding {")
        .nth(1)
        .and_then(|tail| tail.split("pub mod advanced {").next())
        .expect("substrate binding bucket must keep an advanced detail bucket");
    for forbidden in [
        "Channel",
        "ChannelDirection",
        "ChannelKey",
        "ChannelStore",
        "IncomingClassification",
        "TransportOpsError",
    ] {
        assert!(
            !binding_root.contains(forbidden),
            "substrate::binding root must stay on BindingSlot + NoBinding; detail belongs under binding::advanced: {forbidden}"
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
            !substrate_rs.contains(forbidden),
            "substrate surface must not keep deleted in-crate mgmt/epf owners: {forbidden}"
        );
    }
    assert!(
        substrate_rs.contains("TransportEvent, TransportEventKind, TransportMetrics"),
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
fn substrate_allowlist_tracks_core_boundary() {
    let allowlist = compact_ws(&read(".github/allowlists/substrate-public-api.txt"));

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
            "substrate allowlist must track the surviving core surface: {required}"
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
            "substrate allowlist must not keep deleted mgmt/epf buckets: {forbidden}"
        );
    }
    assert!(
        allowlist.contains("TransportEvent, TransportEventKind, TransportMetrics"),
        "substrate allowlist must keep transport event-kind detail in advanced"
    );
}
