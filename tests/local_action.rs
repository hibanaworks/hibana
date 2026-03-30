mod common;
#[path = "support/runtime.rs"]
mod runtime_support;

use common::TestTransport;
use hibana::{
    g::advanced::steps::{ProjectRole, SendStep, StepCons, StepNil},
    g::advanced::{RoleProgram, project},
    g::{self, Msg, Role},
    substrate::{
        SessionId, SessionKit,
        binding::NoBinding,
        runtime::Config,
        wire::{CodecError, WireEncode},
    },
};
use runtime_support::{leak_clock, leak_slab, leak_tap_storage};

#[derive(Clone, Copy)]
struct InstallPayload {
    data: [u8; 4],
}

impl WireEncode for InstallPayload {
    fn encoded_len(&self) -> Option<usize> {
        Some(4)
    }

    fn encode_into(&self, buf: &mut [u8]) -> Result<usize, CodecError> {
        if buf.len() < 4 {
            return Err(CodecError::Truncated);
        }
        buf[..4].copy_from_slice(&self.data);
        Ok(4)
    }
}

const PROGRAM: g::Program<
    StepCons<SendStep<Role<0>, Role<0>, Msg<7, InstallPayload>, 0>, StepNil>,
> = g::send::<Role<0>, Role<0>, Msg<7, InstallPayload>, 0>();

static ACTOR_PROGRAM: RoleProgram<
    'static,
    0,
    <StepCons<SendStep<Role<0>, Role<0>, Msg<7, InstallPayload>, 0>, StepNil> as ProjectRole<
        Role<0>,
    >>::Output,
> = project(&PROGRAM);

fn transport_queue_is_empty(transport: &TestTransport) -> bool {
    transport
        .state
        .lock()
        .expect("state lock")
        .queues
        .values()
        .all(|queue| queue.is_empty())
}

#[tokio::test]
async fn local_action_flow_executes() {
    let tap_buf = leak_tap_storage();
    let slab = leak_slab(1024);
    let config = Config::new(tap_buf, slab);
    let transport = TestTransport::default();
    let cluster: &mut SessionKit<
        'static,
        TestTransport,
        hibana::substrate::runtime::DefaultLabelUniverse,
        hibana::substrate::runtime::CounterClock,
        4,
    > = Box::leak(Box::new(SessionKit::new(leak_clock())));
    let rv_id = cluster
        .add_rendezvous_from_config(config, transport.clone())
        .expect("register rendezvous");

    let sid = SessionId::new(42);

    let endpoint = cluster
        .enter::<0, _, _, _>(rv_id, sid, &ACTOR_PROGRAM, NoBinding)
        .expect("attach actor endpoint");

    // Local action via flow().send() - unified API
    let payload = InstallPayload {
        data: [0x13, 0x37, 0xC0, 0xDE],
    };

    let (endpoint, outcome) = endpoint
        .flow::<Msg<7, InstallPayload>>()
        .expect("flow for local action")
        .send(&payload)
        .await
        .expect("local action succeeded");

    // For non-control messages, outcome is None
    assert!(outcome.is_none());
    // Self-send should NOT transmit over wire
    assert!(transport_queue_is_empty(&transport));

    drop(endpoint);
}
