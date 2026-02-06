mod common;
mod support;

use common::TestTransport;
use hibana::{
    NoBinding,
    endpoint::ControlOutcome,
    g::{
        self, Msg, Role,
        steps::{ProjectRole, SendStep, StepCons, StepNil},
    },
    rendezvous::{Rendezvous, SessionId},
    runtime::{SessionCluster, config::Config},
    transport::wire::{WireEncode, CodecError},
};
use support::{leak_clock, leak_slab, leak_tap_storage};

type Actor = Role<0>;
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

type InstallKeys = Msg<7, InstallPayload>;
type LocalSteps = StepCons<SendStep<Actor, Actor, InstallKeys, 0>, StepNil>;

const PROGRAM: g::Program<LocalSteps> = g::send::<Actor, Actor, InstallKeys, 0>();

type ActorLocal = <LocalSteps as ProjectRole<Actor>>::Output;

static ACTOR_PROGRAM: g::RoleProgram<'static, 0, ActorLocal> =
    g::project::<0, LocalSteps, _>(&PROGRAM);

#[tokio::test]
async fn local_action_flow_executes() {
    let tap_buf = leak_tap_storage();
    let slab = leak_slab(1024);
    let config = Config::new(tap_buf, slab);
    let transport = TestTransport::default();
    let rendezvous: Rendezvous<
        '_,
        '_,
        TestTransport,
        hibana::runtime::consts::DefaultLabelUniverse,
        hibana::runtime::config::CounterClock,
    > = Rendezvous::from_config(config, transport.clone());
    let cluster: &mut SessionCluster<
        'static,
        TestTransport,
        hibana::runtime::consts::DefaultLabelUniverse,
        hibana::runtime::config::CounterClock,
        4,
    > = Box::leak(Box::new(SessionCluster::new(leak_clock())));
    let rv_id = cluster
        .add_rendezvous(rendezvous)
        .expect("register rendezvous");

    let sid = SessionId::new(42);

    let endpoint = cluster
        .attach_cursor::<0, _, _, _>(rv_id, sid, &ACTOR_PROGRAM, NoBinding)
        .expect("attach actor endpoint");

    #[cfg(feature = "test-utils")]
    assert!(endpoint.phase_cursor().is_local_action());

    // Local action via flow().send() - unified API
    let payload = InstallPayload {
        data: [0x13, 0x37, 0xC0, 0xDE],
    };

    let (endpoint, outcome) = endpoint
        .flow::<InstallKeys>()
        .expect("flow for local action")
        .send(&payload)
        .await
        .expect("local action succeeded");

    // For non-control messages, outcome is None
    assert!(matches!(outcome, ControlOutcome::None));
    #[cfg(feature = "test-utils")]
    endpoint.phase_cursor().assert_terminal();

    // Self-send should NOT transmit over wire
    assert!(transport.queue_is_empty());

    drop(endpoint);
}
