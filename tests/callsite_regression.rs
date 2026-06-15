#![cfg(feature = "std")]

mod common;
#[path = "support/runtime.rs"]
mod runtime_support;
#[path = "support/tls_ref.rs"]
mod tls_ref_support;

use core::cell::UnsafeCell;

use common::TestTransport;
use hibana::{
    g::{self, Msg},
    runtime::{
        Config, SessionKitStorage,
        ids::SessionId,
        program::{RoleProgram, project},
        resolver::{DecisionResolution, ResolverError, ResolverRef},
    },
};
use runtime_support::with_runtime_workspace;
use tls_ref_support::with_resident_tls_ref;

type TestKitStorage = SessionKitStorage<'static, TestTransport, 2>;
type ZeroRendezvousKitStorage = SessionKitStorage<'static, TestTransport, 0>;

std::thread_local! {
    static TEST_SLOT: UnsafeCell<TestKitStorage> = const {
        UnsafeCell::new(SessionKitStorage::uninit())
    };
    static ZERO_RENDEZVOUS_SLOT: UnsafeCell<ZeroRendezvousKitStorage> = const {
        UnsafeCell::new(SessionKitStorage::uninit())
    };
}

fn assert_endpoint_operation(error: hibana::EndpointError, operation: &str) {
    assert_eq!(error.operation(), operation);
}

fn assert_attach_operation(error: hibana::runtime::AttachError, operation: &str) {
    assert_eq!(error.operation(), operation);
}

fn assert_resolver_operation(error: ResolverError, operation: &str) {
    assert_eq!(error.operation(), operation);
}

fn project_pair<const MSG_ID: u8>() -> (RoleProgram<0>, RoleProgram<1>) {
    let program = g::send::<0, 1, Msg<MSG_ID, u32>>();
    (project(&program), project(&program))
}

#[test]
fn endpoint_errors_keep_public_operations() {
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        with_resident_tls_ref(&TEST_SLOT, |cluster| {
            let (origin_program, target_program) = project_pair::<11>();
            let config = Config::from_resources(slab);
            let rv = cluster.rendezvous(config, transport.clone()).expect("rv");
            let sid = SessionId::new(411);
            let mut origin = rv
                .session(sid)
                .role(&origin_program)
                .enter()
                .expect("origin");
            let mut target = rv
                .session(sid)
                .role(&target_program)
                .enter()
                .expect("target");

            let flow_error = match origin.flow::<Msg<12, u32>>() {
                Ok(flow) => {
                    drop(flow);
                    panic!("wrong flow label must fail")
                }
                Err(error) => error,
            };
            assert_endpoint_operation(flow_error, "flow");

            futures::executor::block_on(origin.flow::<Msg<11, u32>>().expect("flow").send(&1234))
                .expect("send");

            let recv_result = futures::executor::block_on(target.recv::<Msg<11, u64>>());
            let recv_error = match recv_result {
                Ok(_) => panic!("wrong recv payload must fail"),
                Err(error) => error,
            };
            assert_endpoint_operation(recv_error, "recv");
        });
    });
}

#[test]
fn attach_and_resolver_errors_keep_public_operations() {
    with_runtime_workspace(|slab| {
        with_resident_tls_ref(&ZERO_RENDEZVOUS_SLOT, |cluster| {
            let config = Config::from_resources(slab);
            let rendezvous_result = cluster.rendezvous(config, TestTransport::new());
            let rendezvous_error = match rendezvous_result {
                Ok(_) => panic!("zero-capacity rendezvous table must fail"),
                Err(error) => error,
            };
            assert_attach_operation(rendezvous_error, "rendezvous");
        });
    });

    with_runtime_workspace(|slab| {
        with_resident_tls_ref(&TEST_SLOT, |cluster| {
            let (origin_program, _target_program) = project_pair::<42>();
            let config = Config::from_resources(&mut slab[..4096]);
            let rv = cluster
                .rendezvous(config, TestTransport::new())
                .expect("rv");
            let sid = SessionId::new(441);
            let constrained_role = rv.session(sid).role(&origin_program);
            let enter_result = constrained_role.enter();
            let enter_error = match enter_result {
                Ok(_) => panic!("resource-constrained role enter must fail"),
                Err(error) => error,
            };
            assert_attach_operation(enter_error, "enter");

            let resolver_role = rv.role(&origin_program);
            let resolver = ResolverRef::<77>::decision_fn(|| Ok(DecisionResolution::Defer));
            let set_resolver_result = resolver_role.set_resolver(resolver);
            let set_resolver_error = match set_resolver_result {
                Ok(_) => panic!("resolver without projected site must fail"),
                Err(error) => error,
            };
            assert_resolver_operation(set_resolver_error, "set_resolver");
        });
    });

    let reject_error = ResolverError::reject();
    assert_resolver_operation(reject_error, "reject");
}
