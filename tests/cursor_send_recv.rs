mod common;
#[path = "support/runtime.rs"]
mod runtime_support;
#[path = "support/tls_ref.rs"]
mod tls_ref_support;

use core::{
    cell::UnsafeCell,
    mem::{MaybeUninit, size_of, size_of_val},
};

use common::TestTransport;
use hibana::{
    g::advanced::steps::{SendStep, StepCons, StepNil},
    g::advanced::{RoleProgram, project},
    g::{self, Msg, Role},
    substrate::{
        SessionId, SessionKit,
        binding::NoBinding,
        runtime::{Config, CounterClock},
        wire::{CodecError, Payload, WireEncode, WirePayload},
    },
};
use runtime_support::with_fixture;
use tls_ref_support::with_tls_ref;

const PROGRAM: g::Program<StepCons<SendStep<Role<0>, Role<1>, Msg<1, u32>, 0>, StepNil>> =
    g::send::<Role<0>, Role<1>, Msg<1, u32>, 0>();

#[derive(Clone, Copy)]
struct FramePayload([u8; 4]);

impl WireEncode for FramePayload {
    fn encoded_len(&self) -> Option<usize> {
        Some(self.0.len())
    }

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < self.0.len() {
            return Err(CodecError::Truncated);
        }
        out[..self.0.len()].copy_from_slice(&self.0);
        Ok(self.0.len())
    }
}

impl WirePayload for FramePayload {
    type Decoded<'a> = Payload<'a>;

    fn decode_payload<'a>(input: Payload<'a>) -> Result<Self::Decoded<'a>, CodecError> {
        Ok(input)
    }
}

const BORROWED_PROGRAM: g::Program<
    StepCons<SendStep<Role<0>, Role<1>, Msg<2, FramePayload>, 0>, StepNil>,
> = g::send::<Role<0>, Role<1>, Msg<2, FramePayload>, 0>();

static ORIGIN_PROGRAM: RoleProgram<'static, 0> = project(&PROGRAM);
static TARGET_PROGRAM: RoleProgram<'static, 1> = project(&PROGRAM);
static BORROWED_ORIGIN_PROGRAM: RoleProgram<'static, 0> = project(&BORROWED_PROGRAM);
static BORROWED_TARGET_PROGRAM: RoleProgram<'static, 1> = project(&BORROWED_PROGRAM);
type TestKit = SessionKit<
    'static,
    TestTransport,
    hibana::substrate::runtime::DefaultLabelUniverse,
    CounterClock,
    2,
>;
const ENDPOINT_BYTES_MAX: usize = 8;
const SEND_FUTURE_BYTES_MAX: usize = 304;
const RECV_FUTURE_BYTES_MAX: usize = 88;

std::thread_local! {
    static SESSION_SLOT: UnsafeCell<MaybeUninit<TestKit>> = const {
        UnsafeCell::new(MaybeUninit::uninit())
    };
}

#[test]
fn cursor_recv_can_return_borrowed_frame_views() {
    with_fixture(|clock, tap_buf, slab| {
        let transport = TestTransport::default();
        with_tls_ref(
            &SESSION_SLOT,
            |ptr| unsafe {
                ptr.write(SessionKit::new(clock));
            },
            |cluster| {
                let rv_id = cluster
                    .add_rendezvous_from_config(Config::new(tap_buf, slab), transport.clone())
                    .expect("register rendezvous");

                let sid = SessionId::new(2);
                let mut origin_endpoint = cluster
                    .enter(rv_id, sid, &BORROWED_ORIGIN_PROGRAM, NoBinding)
                    .expect("origin endpoint");
                let mut target_endpoint = cluster
                    .enter(rv_id, sid, &BORROWED_TARGET_PROGRAM, NoBinding)
                    .expect("target endpoint");

                let outcome = futures::executor::block_on(
                    origin_endpoint
                        .flow::<Msg<2, FramePayload>>()
                        .expect("send flow")
                        .send(&FramePayload(*b"hiba")),
                )
                .expect("send succeeds");
                assert!(outcome.is_none());
                let payload =
                    futures::executor::block_on(target_endpoint.recv::<Msg<2, FramePayload>>())
                        .expect("recv succeeds");
                assert_eq!(payload.as_bytes(), b"hiba");
                assert!(transport_queue_is_empty(&transport));
            },
        );
    });
}

fn transport_queue_is_empty(transport: &TestTransport) -> bool {
    transport.queue_is_empty()
}

#[test]
fn cursor_send_and_recv_roundtrip() {
    with_fixture(|clock, tap_buf, slab| {
        let transport = TestTransport::default();
        with_tls_ref(
            &SESSION_SLOT,
            |ptr| unsafe {
                ptr.write(SessionKit::new(clock));
            },
            |cluster| {
                let rv_id = cluster
                    .add_rendezvous_from_config(Config::new(tap_buf, slab), transport.clone())
                    .expect("register rendezvous");

                let sid = SessionId::new(1);
                let mut origin_endpoint = cluster
                    .enter(rv_id, sid, &ORIGIN_PROGRAM, NoBinding)
                    .expect("origin endpoint");
                let mut target_endpoint = cluster
                    .enter(rv_id, sid, &TARGET_PROGRAM, NoBinding)
                    .expect("target endpoint");

                let outcome = futures::executor::block_on(
                    origin_endpoint
                        .flow::<Msg<1, u32>>()
                        .expect("send flow")
                        .send(&42),
                )
                .expect("send succeeds");
                assert!(outcome.is_none());
                let payload = futures::executor::block_on(target_endpoint.recv::<Msg<1, u32>>())
                    .expect("recv succeeds");
                assert_eq!(payload, 42u32);
                assert!(transport_queue_is_empty(&transport));
            },
        );
    });
}

#[test]
fn localside_send_recv_sizes_stay_compact() {
    with_fixture(|clock, tap_buf, slab| {
        let transport = TestTransport::default();
        with_tls_ref(
            &SESSION_SLOT,
            |ptr| unsafe {
                ptr.write(SessionKit::new(clock));
            },
            |cluster| {
                let rv_id = cluster
                    .add_rendezvous_from_config(Config::new(tap_buf, slab), transport)
                    .expect("register rendezvous");

                let sid = SessionId::new(3);
                let mut origin_endpoint = cluster
                    .enter(rv_id, sid, &ORIGIN_PROGRAM, NoBinding)
                    .expect("origin endpoint");
                let mut target_endpoint = cluster
                    .enter(rv_id, sid, &TARGET_PROGRAM, NoBinding)
                    .expect("target endpoint");

                let send = origin_endpoint
                    .flow::<Msg<1, u32>>()
                    .expect("send flow")
                    .send(&42);
                let recv = target_endpoint.recv::<Msg<1, u32>>();

                let endpoint_bytes = size_of::<hibana::Endpoint<'static, 0, TestKit>>();
                let send_future_bytes = size_of_val(&send);
                let recv_future_bytes = size_of_val(&recv);

                assert!(
                    endpoint_bytes <= ENDPOINT_BYTES_MAX,
                    "endpoint handle regressed: {endpoint_bytes} > {ENDPOINT_BYTES_MAX}"
                );
                assert!(
                    send_future_bytes <= SEND_FUTURE_BYTES_MAX,
                    "send future regressed: {send_future_bytes} > {SEND_FUTURE_BYTES_MAX}"
                );
                assert!(
                    recv_future_bytes <= RECV_FUTURE_BYTES_MAX,
                    "recv future regressed: {recv_future_bytes} > {RECV_FUTURE_BYTES_MAX}"
                );

                drop(send);
                drop(recv);
            },
        );
    });
}
