mod common;

use std::mem::size_of_val;

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
    assert!(size_of_val(&role) <= 24);
}
