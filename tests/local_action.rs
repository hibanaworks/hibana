mod common;
#[path = "support/runtime.rs"]
mod runtime_support;
#[path = "support/tls_ref.rs"]
mod tls_ref_support;

use core::{cell::UnsafeCell, mem::MaybeUninit};

use common::TestTransport;
use hibana::{
    g::advanced::steps::{SendStep, StepCons, StepNil},
    g::advanced::{RoleProgram, project},
    g::{self, Msg, Role},
    substrate::{
        SessionId, SessionKit,
        binding::NoBinding,
        runtime::{Config, CounterClock},
        tap::TapEvent,
        wire::{CodecError, WireDecode, WireEncode},
    },
};
use runtime_support::with_fixture;
use tls_ref_support::with_tls_ref;

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

impl<'a> WireDecode<'a> for InstallPayload {
    fn decode_from(input: &'a [u8]) -> Result<Self, CodecError> {
        if input.len() < 4 {
            return Err(CodecError::Truncated);
        }
        let mut data = [0u8; 4];
        data.copy_from_slice(&input[..4]);
        Ok(Self { data })
    }
}

const PROGRAM: g::Program<
    StepCons<SendStep<Role<0>, Role<0>, Msg<7, InstallPayload>, 0>, StepNil>,
> = g::send::<Role<0>, Role<0>, Msg<7, InstallPayload>, 0>();

static ACTOR_PROGRAM: RoleProgram<'static, 0> = project(&PROGRAM);
type TestKit = SessionKit<
    'static,
    TestTransport,
    hibana::substrate::runtime::DefaultLabelUniverse,
    CounterClock,
    2,
>;

std::thread_local! {
    static SESSION_SLOT: UnsafeCell<MaybeUninit<TestKit>> = const {
        UnsafeCell::new(MaybeUninit::uninit())
    };
}

fn transport_queue_is_empty(transport: &TestTransport) -> bool {
    transport.queue_is_empty()
}

fn run_local_action_flow(
    cluster: &'static TestKit,
    tap_buf: &'static mut [TapEvent; runtime_support::RING_EVENTS],
    slab: &'static mut [u8],
    transport: &TestTransport,
) {
    let rv_id = cluster
        .add_rendezvous_from_config(Config::new(tap_buf, slab), transport.clone())
        .expect("register rendezvous");

    let sid = SessionId::new(42);

    let payload = InstallPayload {
        data: [0x13, 0x37, 0xC0, 0xDE],
    };

    let mut endpoint = cluster
        .enter(rv_id, sid, &ACTOR_PROGRAM, NoBinding)
        .expect("attach actor endpoint");
    let outcome = futures::executor::block_on(
        endpoint
            .flow::<Msg<7, InstallPayload>>()
            .expect("install flow")
            .send(&payload),
    )
    .expect("local action succeeded");

    assert!(outcome.is_none());
    assert!(transport_queue_is_empty(transport));
}

#[test]
fn local_action_flow_executes() {
    with_fixture(|clock, tap_buf, slab| {
        let transport = TestTransport::default();
        with_tls_ref(
            &SESSION_SLOT,
            |ptr| unsafe {
                ptr.write(SessionKit::new(clock));
            },
            |cluster| {
                run_local_action_flow(cluster, tap_buf, slab, &transport);
            },
        );
    });
}
