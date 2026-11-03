mod common;
#[path = "../examples/support/in_process.rs"]
pub mod in_process;

use common::TestTransport;
use hibana::{
    g::{self, Msg},
    runtime::program::{RoleProgram, project},
    runtime::{
        SessionKitStorage,
        ids::SessionId,
        resolver::{DecisionArm, ResolverError, ResolverRef},
        wire::{CodecError, Payload, WireEncode, WirePayload},
    },
};
const LOCAL_ROUTE_RESOLVER: u16 = 41;
static LOCAL_ROUTE_STATE: () = ();

struct InvalidUnitSchemaPayload;

impl WireEncode for InvalidUnitSchemaPayload {
    fn encode_into(&self, _out: &mut [u8]) -> Result<usize, CodecError> {
        Ok(1)
    }
}

impl WirePayload for InvalidUnitSchemaPayload {
    const SCHEMA_ID: u32 = 0;

    type Decoded<'a> = Self;

    fn validate_payload(_input: Payload<'_>) -> Result<(), CodecError> {
        Ok(())
    }

    fn decode_validated_payload<'a>(
        _input: hibana::runtime::wire::ReceivedPayload<'a, Self>,
    ) -> Self::Decoded<'a> {
        Self
    }
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn local_action_flow_executes() {
    let transport = TestTransport::new();
    let mut slab = [0; 16 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let cluster = storage.init();
    let program = g::send::<0, 0, Msg<7, ()>>();
    let actor_program: RoleProgram<0> = project(&program);
    let rv = cluster
        .rendezvous(&mut slab, transport.clone())
        .expect("register rendezvous");

    let sid = SessionId::new(42);

    let mut endpoint = rv
        .enter(sid, &actor_program)
        .expect("attach actor endpoint");
    let () = endpoint
        .send::<Msg<7, ()>>(&())
        .await
        .expect("local action succeeded");
    assert!(transport.queue_is_empty());
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn local_action_rejects_nonempty_encoder_claiming_unit_schema() {
    let transport = TestTransport::new();
    let mut slab = [0; 16 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let cluster = storage.init();
    let program = g::send::<0, 0, Msg<7, InvalidUnitSchemaPayload>>();
    let actor_program: RoleProgram<0> = project(&program);
    let rv = cluster
        .rendezvous(&mut slab, transport.clone())
        .expect("register rendezvous");
    let mut endpoint = rv
        .enter(SessionId::new(45), &actor_program)
        .expect("attach actor endpoint");

    let error = endpoint
        .send::<Msg<7, InvalidUnitSchemaPayload>>(&InvalidUnitSchemaPayload)
        .await
        .expect_err("a custom payload cannot impersonate the canonical unit schema");
    let rendered = format!("{error:?}");
    assert!(
        rendered.contains("Codec(Malformed)"),
        "nonempty local encoding must fail closed: {rendered}"
    );
    let poisoned = endpoint
        .send::<Msg<7, InvalidUnitSchemaPayload>>(&InvalidUnitSchemaPayload)
        .await
        .expect_err("invalid unit encoding must poison the session generation");
    assert!(
        format!("{poisoned:?}").contains("SessionFault(ProtocolViolation)"),
        "local schema violation must remain terminal: {poisoned:?}"
    );
    assert!(transport.queue_is_empty());
}

fn local_left(_: &()) -> Result<DecisionArm, ResolverError> {
    Ok(DecisionArm::Left)
}

fn local_route_program<const PAYLOAD_LABEL: u8>() -> RoleProgram<0> {
    let route = g::route(
        g::send::<0, 0, Msg<PAYLOAD_LABEL, ()>>(),
        g::send::<0, 0, Msg<8, ()>>(),
    )
    .resolve::<LOCAL_ROUTE_RESOLVER>();
    project(&route)
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn local_route_recv_accepts_only_empty_payload() {
    let transport = TestTransport::new();
    let mut slab = [0; 16 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let cluster = storage.init();
    let actor_program = local_route_program::<9>();
    let rv = cluster
        .rendezvous(&mut slab, transport.clone())
        .expect("register rendezvous");
    rv.set_resolver(
        &actor_program,
        ResolverRef::<LOCAL_ROUTE_RESOLVER>::decision_state(&LOCAL_ROUTE_STATE, local_left),
    )
    .expect("install resolver");
    let mut endpoint = rv
        .enter(SessionId::new(43), &actor_program)
        .expect("attach actor endpoint");
    let branch = endpoint.offer().await.expect("offer local route");
    assert_eq!(branch.label(), 9);
    let () = branch
        .recv::<Msg<9, ()>>()
        .await
        .expect("empty local branch payload commits");
    assert!(transport.queue_is_empty());
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn local_route_recv_rejects_non_empty_payload() {
    let transport = TestTransport::new();
    let mut slab = [0; 16 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let cluster = storage.init();
    let actor_program = local_route_program::<10>();
    let rv = cluster
        .rendezvous(&mut slab, transport.clone())
        .expect("register rendezvous");
    rv.set_resolver(
        &actor_program,
        ResolverRef::<LOCAL_ROUTE_RESOLVER>::decision_state(&LOCAL_ROUTE_STATE, local_left),
    )
    .expect("install resolver");
    let mut endpoint = rv
        .enter(SessionId::new(44), &actor_program)
        .expect("attach actor endpoint");
    let branch = endpoint.offer().await.expect("offer local route");
    assert_eq!(branch.label(), 10);
    let error = branch
        .recv::<Msg<10, u8>>()
        .await
        .expect_err("local branch receive must match the unit descriptor schema");
    let rendered = format!("{error:?}");
    assert!(
        rendered.contains("PayloadMismatch"),
        "local branch payload mismatch must fail before synthetic payload validation: {rendered}"
    );
    assert!(transport.queue_is_empty());
}
