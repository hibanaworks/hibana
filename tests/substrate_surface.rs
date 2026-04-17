mod common;

use std::fs;
use std::path::PathBuf;

use hibana::g;
use hibana::g::advanced::steps::{SendStep, StepCons, StepNil};
use hibana::g::advanced::{RoleProgram, project};
use hibana::substrate::{
    SessionKit,
    cap::advanced::MintConfig,
    runtime::{CounterClock, DefaultLabelUniverse},
};
use hibana::{Endpoint, RouteBranch};
use static_assertions::assert_not_impl_any;

const PROGRAM: g::Program<StepCons<SendStep<g::Role<0>, g::Role<1>, g::Msg<1, u8>, 0>, StepNil>> =
    g::send::<g::Role<0>, g::Role<1>, g::Msg<1, u8>, 0>();
static CLIENT_PROGRAM: RoleProgram<'static, 0> = project(&PROGRAM);

type StaticTestKit =
    SessionKit<'static, common::TestTransport, DefaultLabelUniverse, CounterClock, 2>;

assert_not_impl_any!(StaticTestKit: Send, Sync);
assert_not_impl_any!(Endpoint<'static, 0, StaticTestKit, MintConfig>: Send, Sync);
assert_not_impl_any!(RouteBranch<'static, 'static, 0, StaticTestKit, MintConfig>: Send, Sync);

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
fn projection_surface_still_builds_after_phase6_split() {
    let _: RoleProgram<'static, 0> = CLIENT_PROGRAM;
}

#[test]
fn substrate_root_exposes_only_phase6_core_buckets() {
    let substrate_rs = read("src/substrate.rs");

    for required in [
        "pub mod runtime {",
        "pub mod tap {",
        "pub mod binding {",
        "pub mod policy {",
        "pub mod cap {",
        "pub mod wire {",
        "pub mod transport {",
        "pub use crate::observe::core::TapEvent;",
        "pub use crate::policy_runtime::PolicySlot;",
        "WirePayload",
    ] {
        assert!(
            substrate_rs.contains(required),
            "substrate surface must keep the phase6 core bucket: {required}"
        );
    }

    for forbidden in [
        "pub mod mgmt {",
        "pub mod epf {",
        "crate::runtime::mgmt",
        "crate::epf",
    ] {
        assert!(
            !substrate_rs.contains(forbidden),
            "substrate surface must not keep deleted in-crate mgmt/epf owners: {forbidden}"
        );
    }
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
fn substrate_allowlist_tracks_phase6_boundary() {
    let allowlist = compact_ws(&read(".github/allowlists/substrate-public-api.txt"));

    for required in [
        "pub mod tap {",
        "pub use crate::observe::core::TapEvent;",
        "pub use crate::policy_runtime::PolicySlot;",
        "WirePayload",
    ] {
        assert!(
            allowlist.contains(required),
            "substrate allowlist must track the surviving phase6 surface: {required}"
        );
    }

    for forbidden in [
        "pub mod mgmt {",
        "pub mod epf {",
        "crate::runtime::mgmt",
        "crate::epf",
    ] {
        assert!(
            !allowlist.contains(forbidden),
            "substrate allowlist must not keep deleted mgmt/epf buckets: {forbidden}"
        );
    }
}
