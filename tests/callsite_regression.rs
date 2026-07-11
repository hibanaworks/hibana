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
const SCHEMA_ROUTE_RESOLVER: u16 = 4120;

fn reject_from_unit(_: &()) -> Result<DecisionArm, ResolverError> {
    Err(ResolverError::reject())
}

fn select_right_from_unit(_: &()) -> Result<DecisionArm, ResolverError> {
    Ok(DecisionArm::Right)
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
            let target = rv.enter(sid, &target_program).expect("target");

            let send_error = futures::executor::block_on(origin.send::<Msg<12, u32>>(&1234))
                .expect_err("wrong send label must fail");
            assert_endpoint_debug_boundary(send_error, "send");
            let poisoned = futures::executor::block_on(origin.send::<Msg<11, u32>>(&1234))
                .expect_err("wrong send label must poison the session");
            assert!(format!("{poisoned:?}").contains("SessionFault(ProtocolViolation)"));

            drop(origin);
            drop(target);
            let sid = SessionId::new(412);
            let mut origin = rv.enter(sid, &origin_program).expect("fresh origin");
            let mut target = rv.enter(sid, &target_program).expect("fresh target");
            futures::executor::block_on(origin.send::<Msg<11, u32>>(&1234)).expect("send");

            let recv_result = futures::executor::block_on(target.recv::<Msg<11, i32>>());
            let recv_error = match recv_result {
                Ok(_) => panic!("wrong recv payload must fail"),
                Err(error) => error,
            };
            assert_endpoint_debug_boundary(recv_error, "recv");
        });
    });
}

#[test]
fn payload_schema_mismatch_cannot_publish_send_or_consume_recv() {
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        with_resident_tls_ref(&TEST_SLOT, |cluster| {
            let (origin_program, target_program) = project_pair::<13>();
            let rv = cluster
                .rendezvous(slab, transport.clone())
                .expect("rendezvous");
            let sid = SessionId::new(412);
            let mut origin = rv.enter(sid, &origin_program).expect("origin");
            let target = rv.enter(sid, &target_program).expect("target");

            let send_error = futures::executor::block_on(origin.send::<Msg<13, i32>>(&1234_i32))
                .expect_err("same-width wrong send schema must fail before publication");
            assert!(format!("{send_error:?}").contains("SchemaMismatch"));
            assert!(transport.queue_is_empty());

            let poisoned = futures::executor::block_on(origin.send::<Msg<13, u32>>(&1234_u32))
                .expect_err("wrong send schema must poison the session");
            assert!(format!("{poisoned:?}").contains("SessionFault(ProtocolViolation)"));
            assert!(transport.queue_is_empty());

            drop(origin);
            drop(target);
            let sid = SessionId::new(414);
            let mut origin = rv.enter(sid, &origin_program).expect("fresh origin");
            let mut target = rv.enter(sid, &target_program).expect("fresh target");
            futures::executor::block_on(origin.send::<Msg<13, u32>>(&1234_u32))
                .expect("fresh session must publish the declared payload");
            assert!(!transport.queue_is_empty());

            let recv_error = futures::executor::block_on(target.recv::<Msg<13, i32>>())
                .expect_err("same-width wrong recv schema must fail before frame consumption");
            assert!(format!("{recv_error:?}").contains("SchemaMismatch"));
            assert!(
                !transport.queue_is_empty(),
                "schema rejection must leave the queued frame unconsumed"
            );
            let poisoned = futures::executor::block_on(target.recv::<Msg<13, u32>>())
                .expect_err("wrong recv schema must poison the session");
            assert!(format!("{poisoned:?}").contains("SessionFault(ProtocolViolation)"));
        });
    });
}

#[test]
fn payload_schema_tracks_selected_route_when_labels_match() {
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        with_resident_tls_ref(&TEST_SLOT, |cluster| {
            let program = g::route(
                g::send::<0, 1, Msg<14, u32>>(),
                g::send::<0, 1, Msg<14, i32>>(),
            )
            .resolve::<SCHEMA_ROUTE_RESOLVER>();
            let origin_program: RoleProgram<0> = project(&program);
            let target_program: RoleProgram<1> = project(&program);
            let rv = cluster
                .rendezvous(slab, transport.clone())
                .expect("rendezvous");
            rv.set_resolver(
                &origin_program,
                ResolverRef::<SCHEMA_ROUTE_RESOLVER>::decision_state(
                    &UNIT_RESOLVER_STATE,
                    select_right_from_unit,
                ),
            )
            .expect("install schema route resolver");
            let sid = SessionId::new(413);
            let mut origin = rv.enter(sid, &origin_program).expect("origin");
            let target = rv.enter(sid, &target_program).expect("target");

            let mismatch = futures::executor::block_on(origin.send::<Msg<14, u32>>(&23))
                .expect_err("selected right arm must reject the left payload schema");
            assert!(format!("{mismatch:?}").contains("SchemaMismatch"));
            assert!(transport.queue_is_empty());

            let poisoned = futures::executor::block_on(origin.send::<Msg<14, i32>>(&-17))
                .expect_err("wrong selected-arm schema must poison the session");
            assert!(format!("{poisoned:?}").contains("SessionFault(ProtocolViolation)"));

            drop(origin);
            drop(target);
            let sid = SessionId::new(414);
            let mut origin = rv.enter(sid, &origin_program).expect("fresh origin");
            let mut target = rv.enter(sid, &target_program).expect("fresh target");
            futures::executor::block_on(origin.send::<Msg<14, i32>>(&-17))
                .expect("selected right arm and payload schema must agree");
            let branch = futures::executor::block_on(target.offer()).expect("offer right arm");
            assert_eq!(branch.label(), 14);
            assert_eq!(
                futures::executor::block_on(branch.recv::<Msg<14, i32>>())
                    .expect("branch receive schema must match the selected arm"),
                -17
            );
            assert!(transport.queue_is_empty());
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
