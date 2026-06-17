mod common;
#[path = "support/runtime.rs"]
mod runtime_support;
#[path = "support/tls_ref.rs"]
mod tls_ref_support;

use core::cell::UnsafeCell;

use common::TestTransport;
use hibana::{
    g::{self, Msg},
    runtime::program::{RoleProgram, project},
    runtime::{
        SessionKit, SessionKitStorage,
        ids::SessionId,
        resolver::{DecisionArm, ResolverError, ResolverRef},
        wire::{CodecError, Payload, WireEncode, WirePayload},
    },
};
use runtime_support::with_runtime_workspace;
use tls_ref_support::with_resident_tls_ref;

#[derive(Clone, Copy)]
struct InstallPayload {
    data: [u8; 4],
}

impl WireEncode for InstallPayload {
    fn encode_into(&self, buf: &mut [u8]) -> Result<usize, CodecError> {
        if buf.len() < 4 {
            return Err(CodecError::Truncated);
        }
        buf[..4].copy_from_slice(&self.data);
        Ok(4)
    }
}

impl WirePayload for InstallPayload {
    type Decoded<'a> = Self;

    fn validate_payload(input: Payload<'_>) -> Result<(), CodecError> {
        if input.as_bytes().len() == 4 {
            Ok(())
        } else if input.as_bytes().len() < 4 {
            Err(CodecError::Truncated)
        } else {
            Err(CodecError::Malformed)
        }
    }

    fn decode_validated_payload<'a>(input: Payload<'a>) -> Self::Decoded<'a> {
        let input = input.as_bytes();
        let mut data = [0u8; 4];
        data.copy_from_slice(&input[..4]);
        Self { data }
    }
}

type TestKit = SessionKit<'static, TestTransport>;
type TestKitStorage = SessionKitStorage<'static, TestTransport>;
const LOCAL_ROUTE_RESOLVER: u16 = 41;
static LOCAL_ROUTE_STATE: () = ();

std::thread_local! {
    static SESSION_SLOT: UnsafeCell<TestKitStorage> = const {
        UnsafeCell::new(SessionKitStorage::uninit())
    };
}

fn transport_queue_is_empty(transport: &TestTransport) -> bool {
    transport.queue_is_empty()
}

fn run_local_action_flow(
    cluster: &'static TestKit,
    slab: &'static mut [u8],
    transport: &TestTransport,
) {
    let program = g::send::<0, 0, Msg<7, InstallPayload>>();
    let actor_program: RoleProgram<0> = project(&program);
    let rv = cluster
        .rendezvous(slab, transport.clone())
        .expect("register rendezvous");

    let sid = SessionId::new(42);

    let payload = InstallPayload {
        data: [0x13, 0x37, 0xC0, 0xDE],
    };

    let mut endpoint = rv
        .enter(sid, &actor_program)
        .expect("attach actor endpoint");
    let () = futures::executor::block_on(endpoint.send::<Msg<7, InstallPayload>>(&payload))
        .expect("local action succeeded");
    assert!(transport_queue_is_empty(transport));
}

fn local_left(_: &()) -> Result<DecisionArm, ResolverError> {
    Ok(DecisionArm::Left)
}

fn local_route_program<const PAYLOAD_LABEL: u8, P>() -> RoleProgram<0> {
    let route = g::route(
        g::send::<0, 0, Msg<PAYLOAD_LABEL, P>>(),
        g::send::<0, 0, Msg<8, ()>>(),
    )
    .resolve::<LOCAL_ROUTE_RESOLVER>();
    project(&route)
}

fn run_local_route_recv_empty_payload(
    cluster: &'static TestKit,
    slab: &'static mut [u8],
    transport: &TestTransport,
) {
    let actor_program = local_route_program::<9, ()>();
    let rv = cluster
        .rendezvous(slab, transport.clone())
        .expect("register rendezvous");
    rv.set_resolver(
        &actor_program,
        ResolverRef::<LOCAL_ROUTE_RESOLVER>::decision_state(&LOCAL_ROUTE_STATE, local_left),
    )
    .expect("install resolver");
    let mut endpoint = rv
        .enter(SessionId::new(43), &actor_program)
        .expect("attach actor endpoint");
    let branch = futures::executor::block_on(endpoint.offer()).expect("offer local route");
    assert_eq!(branch.label(), 9);
    let () = futures::executor::block_on(branch.recv::<Msg<9, ()>>())
        .expect("empty local branch payload commits");
    assert!(transport_queue_is_empty(transport));
}

fn run_local_route_recv_non_empty_payload_fails_closed(
    cluster: &'static TestKit,
    slab: &'static mut [u8],
    transport: &TestTransport,
) {
    let actor_program = local_route_program::<10, u8>();
    let rv = cluster
        .rendezvous(slab, transport.clone())
        .expect("register rendezvous");
    rv.set_resolver(
        &actor_program,
        ResolverRef::<LOCAL_ROUTE_RESOLVER>::decision_state(&LOCAL_ROUTE_STATE, local_left),
    )
    .expect("install resolver");
    let mut endpoint = rv
        .enter(SessionId::new(44), &actor_program)
        .expect("attach actor endpoint");
    let branch = futures::executor::block_on(endpoint.offer()).expect("offer local route");
    assert_eq!(branch.label(), 10);
    let error = futures::executor::block_on(branch.recv::<Msg<10, u8>>())
        .expect_err("non-empty local branch payload must fail closed");
    let rendered = format!("{error:?}");
    assert!(
        rendered.contains("Truncated") || rendered.contains("Codec"),
        "non-empty local branch payload must fail through payload validation: {rendered}"
    );
    assert!(transport_queue_is_empty(transport));
}

#[test]
fn local_action_flow_executes() {
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            run_local_action_flow(cluster, slab, &transport);
        });
    });
}

#[test]
fn local_route_recv_accepts_only_empty_payload() {
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            run_local_route_recv_empty_payload(cluster, slab, &transport);
        });
    });
}

#[test]
fn local_route_recv_rejects_non_empty_payload() {
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            run_local_route_recv_non_empty_payload_fails_closed(cluster, slab, &transport);
        });
    });
}
