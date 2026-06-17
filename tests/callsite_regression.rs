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
        SessionKitStorage,
        ids::SessionId,
        program::{RoleProgram, project},
        resolver::{DecisionArm, ResolverError, ResolverRef},
    },
};
use runtime_support::with_runtime_workspace;
use tls_ref_support::with_resident_tls_ref;

type TestKitStorage = SessionKitStorage<'static, TestTransport>;

std::thread_local! {
    static TEST_SLOT: UnsafeCell<TestKitStorage> = const {
        UnsafeCell::new(SessionKitStorage::uninit())
    };
}

fn assert_endpoint_debug_boundary(error: hibana::EndpointError, operation: &str) {
    let rendered = format!("{error:?}");
    assert!(
        rendered.contains(&format!("operation: \"{operation}\"")),
        "EndpointError Debug must include operation {operation}: {error:?}"
    );
    assert_compact_debug(&rendered);
}

fn assert_attach_debug_boundary(error: hibana::runtime::AttachError, operation: &str) {
    let rendered = format!("{error:?}");
    assert!(
        rendered.contains(&format!("operation: \"{operation}\"")),
        "AttachError Debug must include operation {operation}: {error:?}"
    );
    assert_compact_debug(&rendered);
}

fn assert_resolver_debug_boundary(error: ResolverError, operation: &str) {
    let rendered = format!("{error:?}");
    assert!(
        rendered.contains(&format!("operation: \"{operation}\"")),
        "ResolverError Debug must include operation {operation}: {error:?}"
    );
    assert_compact_debug(&rendered);
}

fn assert_compact_debug(rendered: &str) {
    for forbidden in ["file", "line", "column"] {
        assert!(
            !rendered.contains(forbidden),
            "public error Debug must not carry source location {forbidden}: {rendered}"
        );
    }
}

static UNIT_RESOLVER_STATE: () = ();

fn reject_from_unit(_: &()) -> Result<DecisionArm, ResolverError> {
    Err(ResolverError::reject())
}

fn project_pair<const MSG_ID: u8>() -> (RoleProgram<0>, RoleProgram<1>) {
    let program = g::send::<0, 1, Msg<MSG_ID, u32>>();
    (project(&program), project(&program))
}

#[test]
fn runtime_registers_multiple_rendezvous_from_resident_resources() {
    with_runtime_workspace(|slab| {
        with_resident_tls_ref(&TEST_SLOT, |cluster| {
            let (role0, _) = project_pair::<7>();
            let transport = TestTransport::new();
            const CHUNK: usize = 16 * 1024;
            let (chunk0, rest) = slab.split_at_mut(CHUNK);
            let (chunk1, rest) = rest.split_at_mut(CHUNK);
            let (chunk2, rest) = rest.split_at_mut(CHUNK);
            let (chunk3, rest) = rest.split_at_mut(CHUNK);
            let (chunk4, rest) = rest.split_at_mut(CHUNK);
            let (chunk5, _) = rest.split_at_mut(CHUNK);
            for (idx, chunk) in [chunk0, chunk1, chunk2, chunk3, chunk4, chunk5]
                .into_iter()
                .enumerate()
            {
                let rv = cluster
                    .rendezvous(chunk, transport.clone())
                    .expect("runtime registry must grow from each resident rendezvous resource");
                let endpoint = rv
                    .enter(SessionId::new(700 + idx as u32), &role0)
                    .expect("wide rendezvous budget must still attach endpoints");
                core::hint::black_box(&endpoint);
                drop(endpoint);
            }
        });
    });
}

#[test]
fn endpoint_errors_keep_debug_boundaries() {
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        with_resident_tls_ref(&TEST_SLOT, |cluster| {
            let (origin_program, target_program) = project_pair::<11>();
            let rv = cluster.rendezvous(slab, transport.clone()).expect("rv");
            let sid = SessionId::new(411);
            let mut origin = rv.enter(sid, &origin_program).expect("origin");
            let mut target = rv.enter(sid, &target_program).expect("target");

            let send_error = futures::executor::block_on(origin.send::<Msg<12, u32>>(&1234))
                .expect_err("wrong send label must fail");
            assert_endpoint_debug_boundary(send_error, "send");

            futures::executor::block_on(origin.send::<Msg<11, u32>>(&1234)).expect("send");

            let recv_result = futures::executor::block_on(target.recv::<Msg<11, u64>>());
            let recv_error = match recv_result {
                Ok(_) => panic!("wrong recv payload must fail"),
                Err(error) => error,
            };
            assert_endpoint_debug_boundary(recv_error, "recv");
        });
    });
}

#[test]
fn attach_and_resolver_errors_keep_debug_boundaries() {
    with_runtime_workspace(|slab| {
        with_resident_tls_ref(&TEST_SLOT, |cluster| {
            let rendezvous_result = cluster.rendezvous(&mut slab[..1], TestTransport::new());
            let rendezvous_error = match rendezvous_result {
                Ok(_) => panic!("resource-constrained rendezvous slab must fail"),
                Err(error) => error,
            };
            assert_attach_debug_boundary(rendezvous_error, "rendezvous");
        });
    });

    with_runtime_workspace(|slab| {
        with_resident_tls_ref(&TEST_SLOT, |cluster| {
            let (origin_program, _target_program) = project_pair::<42>();
            let rv = cluster
                .rendezvous(&mut slab[..2048], TestTransport::new())
                .expect("rv");
            let sid = SessionId::new(441);
            let enter_result = rv.enter(sid, &origin_program);
            let enter_error = match enter_result {
                Ok(_) => panic!("resource-constrained role enter must fail"),
                Err(error) => error,
            };
            assert_attach_debug_boundary(enter_error, "enter");

            let resolver =
                ResolverRef::<77>::decision_state(&UNIT_RESOLVER_STATE, reject_from_unit);
            let set_resolver_result = rv.set_resolver(&origin_program, resolver);
            let set_resolver_error = match set_resolver_result {
                Ok(_) => panic!("resolver without projected site must fail"),
                Err(error) => error,
            };
            assert_resolver_debug_boundary(set_resolver_error, "set_resolver");
        });
    });

    let reject_error = ResolverError::reject();
    assert_resolver_debug_boundary(reject_error, "reject");
}
