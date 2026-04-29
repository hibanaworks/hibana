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
    g::{self, Msg, Role},
    substrate::program::{RoleProgram, project},
    substrate::{
        SessionKit,
        binding::NoBinding,
        cap::{
            CapShot, ControlResourceKind, GenericCapToken, ResourceKind,
            advanced::{
                CAP_HANDLE_LEN, CapError, CapHeader, ControlOp, ControlPath, ControlScopeKind,
                ScopeId,
            },
        },
        ids::SessionId,
        runtime::{Config, CounterClock, LabelUniverse},
        wire::{CodecError, Payload, WireEncode, WirePayload},
    },
};
use runtime_support::with_fixture;
use tls_ref_support::with_tls_ref;

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

const MANUAL_WIRE_CONTROL_LOGICAL: u8 = 122;
const MANUAL_WIRE_ABORT_ACK_LOGICAL: u8 = 123;
const MANUAL_WIRE_ONE_SHOT_ABORT_ACK_LOGICAL: u8 = 124;
const ABORT_ACK_ID: u16 = 0x0201;
const MANUAL_TOKEN_NONCE_LEN: usize = 16;
const MANUAL_TOKEN_HEADER_LEN: usize = 40;
const MANUAL_TOKEN_TAG_LEN: usize = 16;
const MANUAL_TOKEN_LEN: usize =
    MANUAL_TOKEN_NONCE_LEN + MANUAL_TOKEN_HEADER_LEN + MANUAL_TOKEN_TAG_LEN;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ManualWireControl;

impl ResourceKind for ManualWireControl {
    type Handle = (u32, u16);
    const TAG: u8 = 0x72;
    const NAME: &'static str = "ManualWireControl";

    fn encode_handle(handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN] {
        let mut out = [0u8; CAP_HANDLE_LEN];
        out[0..4].copy_from_slice(&handle.0.to_be_bytes());
        out[4..6].copy_from_slice(&handle.1.to_be_bytes());
        out
    }

    fn decode_handle(data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError> {
        Ok((
            u32::from_be_bytes([data[0], data[1], data[2], data[3]]),
            u16::from_be_bytes([data[4], data[5]]),
        ))
    }

    fn zeroize(_handle: &mut Self::Handle) {}
}

impl ControlResourceKind for ManualWireControl {
    const SCOPE: ControlScopeKind = ControlScopeKind::Policy;
    const PATH: ControlPath = ControlPath::Wire;
    const TAP_ID: u16 = 0x0472;
    const SHOT: CapShot = CapShot::Many;
    const OP: ControlOp = ControlOp::Fence;
    const AUTO_MINT_WIRE: bool = false;

    fn mint_handle(
        session: SessionId,
        lane: hibana::substrate::ids::Lane,
        _scope: ScopeId,
    ) -> Self::Handle {
        (session.raw(), lane.as_wire() as u16)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ManualWireAbortAckControl;

impl ResourceKind for ManualWireAbortAckControl {
    type Handle = (u32, u16);
    const TAG: u8 = 0x74;
    const NAME: &'static str = "ManualWireAbortAckControl";

    fn encode_handle(handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN] {
        let mut out = [0u8; CAP_HANDLE_LEN];
        out[0..4].copy_from_slice(&handle.0.to_le_bytes());
        out[4..6].copy_from_slice(&handle.1.to_le_bytes());
        out
    }

    fn decode_handle(data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError> {
        Ok((
            u32::from_le_bytes([data[0], data[1], data[2], data[3]]),
            u16::from_le_bytes([data[4], data[5]]),
        ))
    }

    fn zeroize(_handle: &mut Self::Handle) {}
}

impl ControlResourceKind for ManualWireAbortAckControl {
    const SCOPE: ControlScopeKind = ControlScopeKind::Abort;
    const PATH: ControlPath = ControlPath::Wire;
    const TAP_ID: u16 = ABORT_ACK_ID;
    const SHOT: CapShot = CapShot::Many;
    const OP: ControlOp = ControlOp::AbortAck;
    const AUTO_MINT_WIRE: bool = false;

    fn mint_handle(
        session: SessionId,
        lane: hibana::substrate::ids::Lane,
        _scope: ScopeId,
    ) -> Self::Handle {
        (session.raw(), lane.as_wire() as u16)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ManualWireOneShotAbortAckControl;

impl ResourceKind for ManualWireOneShotAbortAckControl {
    type Handle = (u32, u16);
    const TAG: u8 = 0x75;
    const NAME: &'static str = "ManualWireOneShotAbortAckControl";

    fn encode_handle(handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN] {
        let mut out = [0u8; CAP_HANDLE_LEN];
        out[0..4].copy_from_slice(&handle.0.to_le_bytes());
        out[4..6].copy_from_slice(&handle.1.to_le_bytes());
        out
    }

    fn decode_handle(data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError> {
        Ok((
            u32::from_le_bytes([data[0], data[1], data[2], data[3]]),
            u16::from_le_bytes([data[4], data[5]]),
        ))
    }

    fn zeroize(_handle: &mut Self::Handle) {}
}

impl ControlResourceKind for ManualWireOneShotAbortAckControl {
    const SCOPE: ControlScopeKind = ControlScopeKind::Abort;
    const PATH: ControlPath = ControlPath::Wire;
    const TAP_ID: u16 = ABORT_ACK_ID;
    const SHOT: CapShot = CapShot::One;
    const OP: ControlOp = ControlOp::AbortAck;
    const AUTO_MINT_WIRE: bool = false;

    fn mint_handle(
        session: SessionId,
        lane: hibana::substrate::ids::Lane,
        _scope: ScopeId,
    ) -> Self::Handle {
        (session.raw(), lane.as_wire() as u16)
    }
}

fn manual_wire_token(
    sid: SessionId,
    lane: hibana::substrate::ids::Lane,
    peer: u8,
) -> GenericCapToken<ManualWireControl> {
    let handle = ManualWireControl::encode_handle(&(sid.raw(), lane.as_wire() as u16));
    let mut header = [0u8; MANUAL_TOKEN_HEADER_LEN];
    CapHeader::new(
        sid,
        lane,
        peer,
        ManualWireControl::TAG,
        ManualWireControl::OP,
        ManualWireControl::PATH,
        ManualWireControl::SHOT,
        ManualWireControl::SCOPE,
        0,
        ScopeId::generic(0).local_ordinal(),
        0,
        handle,
    )
    .encode(&mut header);

    let mut bytes = [0u8; MANUAL_TOKEN_LEN];
    bytes[..MANUAL_TOKEN_NONCE_LEN].copy_from_slice(&[0xAB; MANUAL_TOKEN_NONCE_LEN]);
    bytes[MANUAL_TOKEN_NONCE_LEN..MANUAL_TOKEN_NONCE_LEN + MANUAL_TOKEN_HEADER_LEN]
        .copy_from_slice(&header);
    bytes[MANUAL_TOKEN_NONCE_LEN + MANUAL_TOKEN_HEADER_LEN..MANUAL_TOKEN_LEN]
        .copy_from_slice(&[0u8; MANUAL_TOKEN_TAG_LEN]);
    GenericCapToken::from_bytes(bytes)
}

fn manual_wire_abort_ack_token(
    sid: SessionId,
    lane: hibana::substrate::ids::Lane,
    peer: u8,
    scope_id: u16,
    epoch: u16,
) -> GenericCapToken<ManualWireAbortAckControl> {
    manual_wire_abort_ack_token_for::<ManualWireAbortAckControl>(
        sid,
        lane,
        peer,
        scope_id,
        epoch,
        sid.raw(),
        lane.as_wire() as u16,
    )
}

fn manual_wire_abort_ack_token_with_handle(
    sid: SessionId,
    lane: hibana::substrate::ids::Lane,
    peer: u8,
    scope_id: u16,
    epoch: u16,
    handle_sid: u32,
    handle_lane: u16,
) -> GenericCapToken<ManualWireAbortAckControl> {
    manual_wire_abort_ack_token_for::<ManualWireAbortAckControl>(
        sid,
        lane,
        peer,
        scope_id,
        epoch,
        handle_sid,
        handle_lane,
    )
}

fn manual_wire_one_shot_abort_ack_token(
    sid: SessionId,
    lane: hibana::substrate::ids::Lane,
    peer: u8,
    scope_id: u16,
    epoch: u16,
) -> GenericCapToken<ManualWireOneShotAbortAckControl> {
    manual_wire_abort_ack_token_for::<ManualWireOneShotAbortAckControl>(
        sid,
        lane,
        peer,
        scope_id,
        epoch,
        sid.raw(),
        lane.as_wire() as u16,
    )
}

fn manual_wire_abort_ack_token_for<K>(
    sid: SessionId,
    lane: hibana::substrate::ids::Lane,
    peer: u8,
    scope_id: u16,
    epoch: u16,
    handle_sid: u32,
    handle_lane: u16,
) -> GenericCapToken<K>
where
    K: ControlResourceKind + ResourceKind<Handle = (u32, u16)>,
{
    let handle = K::encode_handle(&(handle_sid, handle_lane));
    let mut header = [0u8; MANUAL_TOKEN_HEADER_LEN];
    CapHeader::new(
        sid,
        lane,
        peer,
        K::TAG,
        K::OP,
        K::PATH,
        K::SHOT,
        K::SCOPE,
        0,
        scope_id,
        epoch,
        handle,
    )
    .encode(&mut header);

    let mut bytes = [0u8; MANUAL_TOKEN_LEN];
    bytes[..MANUAL_TOKEN_NONCE_LEN].copy_from_slice(&[0xCD; MANUAL_TOKEN_NONCE_LEN]);
    bytes[MANUAL_TOKEN_NONCE_LEN..MANUAL_TOKEN_NONCE_LEN + MANUAL_TOKEN_HEADER_LEN]
        .copy_from_slice(&header);
    bytes[MANUAL_TOKEN_NONCE_LEN + MANUAL_TOKEN_HEADER_LEN..MANUAL_TOKEN_LEN]
        .copy_from_slice(&[0u8; MANUAL_TOKEN_TAG_LEN]);
    GenericCapToken::from_bytes(bytes)
}

type TestKit = SessionKit<
    'static,
    TestTransport,
    hibana::substrate::runtime::DefaultLabelUniverse,
    CounterClock,
    2,
>;

#[derive(Clone, Copy, Debug, Default)]
struct LowLabelUniverse;

impl LabelUniverse for LowLabelUniverse {
    const MAX_LABEL: u8 = 127;
}

type LowLabelKit = SessionKit<'static, TestTransport, LowLabelUniverse, CounterClock, 2>;

// `Endpoint<'r, ROLE>` is already role-only opaque. Keep the measured bound
// tighter than the public v3 contract (`<= 40`) so regressions trip early even
// before the remaining future/branch compression lands.
const ENDPOINT_BYTES_MAX: usize = 24;
const SEND_FUTURE_BYTES_MAX: usize = 48;
const RECV_FUTURE_BYTES_MAX: usize = 48;

std::thread_local! {
    static SESSION_SLOT: UnsafeCell<MaybeUninit<TestKit>> = const {
        UnsafeCell::new(MaybeUninit::uninit())
    };
    static LOW_LABEL_SESSION_SLOT: UnsafeCell<MaybeUninit<LowLabelKit>> = const {
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
                let borrowed_program = g::send::<Role<0>, Role<1>, Msg<2, FramePayload>, 0>();
                let borrowed_origin_program: RoleProgram<0> = project(&borrowed_program);
                let borrowed_target_program: RoleProgram<1> = project(&borrowed_program);
                let rv_id = cluster
                    .add_rendezvous_from_config(Config::new(tap_buf, slab), transport.clone())
                    .expect("register rendezvous");

                let sid = SessionId::new(2);
                let mut origin_endpoint = cluster
                    .enter(rv_id, sid, &borrowed_origin_program, NoBinding)
                    .expect("origin endpoint");
                let mut target_endpoint = cluster
                    .enter(rv_id, sid, &borrowed_target_program, NoBinding)
                    .expect("target endpoint");

                let () = futures::executor::block_on(
                    origin_endpoint
                        .flow::<Msg<2, FramePayload>>()
                        .expect("send flow")
                        .send(&FramePayload(*b"hiba")),
                )
                .expect("send succeeds");
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

fn assert_manual_wire_abort_ack_send_rejected(
    token: GenericCapToken<ManualWireAbortAckControl>,
    sid: SessionId,
) {
    with_fixture(|clock, tap_buf, slab| {
        let transport = TestTransport::default();
        let tap_ptr = tap_buf as *mut _;
        with_tls_ref(
            &SESSION_SLOT,
            |ptr| unsafe {
                ptr.write(SessionKit::new(clock));
            },
            |cluster| {
                let program = g::send::<
                    Role<0>,
                    Role<1>,
                    Msg<
                        { MANUAL_WIRE_ABORT_ACK_LOGICAL },
                        GenericCapToken<ManualWireAbortAckControl>,
                        ManualWireAbortAckControl,
                    >,
                    0,
                >();
                let origin_program: RoleProgram<0> = project(&program);
                let target_program: RoleProgram<1> = project(&program);
                let rv_id = cluster
                    .add_rendezvous_from_config(
                        Config::new(unsafe { &mut *tap_ptr }, slab),
                        transport.clone(),
                    )
                    .expect("register rendezvous");

                let mut origin_endpoint = cluster
                    .enter(rv_id, sid, &origin_program, NoBinding)
                    .expect("origin endpoint");
                let target_endpoint = cluster
                    .enter(rv_id, sid, &target_program, NoBinding)
                    .expect("target endpoint");
                core::hint::black_box(&target_endpoint);

                let err = futures::executor::block_on(
                    origin_endpoint
                        .flow::<Msg<
                            { MANUAL_WIRE_ABORT_ACK_LOGICAL },
                            GenericCapToken<ManualWireAbortAckControl>,
                            ManualWireAbortAckControl,
                        >>()
                        .expect("wire abort-ack flow")
                        .send(&token),
                )
                .expect_err("bound mismatch must fail before transport");

                assert!(matches!(err, hibana::SendError::PhaseInvariant));
                assert!(transport_queue_is_empty(&transport));
            },
        );
        assert!(
            !unsafe { &*tap_ptr }
                .iter()
                .any(|event| event.id == ABORT_ACK_ID),
            "rejected explicit control send must not execute abort-ack",
        );
    });
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
                let program = g::send::<Role<0>, Role<1>, Msg<1, u32>, 0>();
                let origin_program: RoleProgram<0> = project(&program);
                let target_program: RoleProgram<1> = project(&program);
                let rv_id = cluster
                    .add_rendezvous_from_config(Config::new(tap_buf, slab), transport.clone())
                    .expect("register rendezvous");

                let sid = SessionId::new(1);
                let mut origin_endpoint = cluster
                    .enter(rv_id, sid, &origin_program, NoBinding)
                    .expect("origin endpoint");
                let mut target_endpoint = cluster
                    .enter(rv_id, sid, &target_program, NoBinding)
                    .expect("target endpoint");

                let () = futures::executor::block_on(
                    origin_endpoint
                        .flow::<Msg<1, u32>>()
                        .expect("send flow")
                        .send(&42),
                )
                .expect("send succeeds");
                let payload = futures::executor::block_on(target_endpoint.recv::<Msg<1, u32>>())
                    .expect("recv succeeds");
                assert_eq!(payload, 42u32);
                assert!(transport_queue_is_empty(&transport));
            },
        );
    });
}

#[test]
fn cursor_send_and_recv_high_logical_label_roundtrip() {
    with_fixture(|clock, tap_buf, slab| {
        let transport = TestTransport::default();
        with_tls_ref(
            &SESSION_SLOT,
            |ptr| unsafe {
                ptr.write(SessionKit::new(clock));
            },
            |cluster| {
                let program = g::send::<Role<0>, Role<1>, Msg<200, u32>, 0>();
                let origin_program: RoleProgram<0> = project(&program);
                let target_program: RoleProgram<1> = project(&program);
                let rv_id = cluster
                    .add_rendezvous_from_config(Config::new(tap_buf, slab), transport.clone())
                    .expect("register rendezvous");

                let sid = SessionId::new(200);
                let mut origin_endpoint = cluster
                    .enter(rv_id, sid, &origin_program, NoBinding)
                    .expect("origin endpoint");
                let mut target_endpoint = cluster
                    .enter(rv_id, sid, &target_program, NoBinding)
                    .expect("target endpoint");

                let () = futures::executor::block_on(
                    origin_endpoint
                        .flow::<Msg<200, u32>>()
                        .expect("send flow")
                        .send(&0xC8C8_C8C8),
                )
                .expect("send succeeds");
                let payload = futures::executor::block_on(target_endpoint.recv::<Msg<200, u32>>())
                    .expect("recv succeeds");
                assert_eq!(payload, 0xC8C8_C8C8);
                assert!(transport_queue_is_empty(&transport));
            },
        );
    });
}

#[test]
fn custom_label_universe_rejects_high_logical_label_on_enter() {
    with_fixture(|clock, tap_buf, slab| {
        let transport = TestTransport::default();
        with_tls_ref(
            &LOW_LABEL_SESSION_SLOT,
            |ptr| unsafe {
                ptr.write(SessionKit::new(clock));
            },
            |cluster| {
                let program = g::send::<Role<0>, Role<1>, Msg<200, u32>, 0>();
                let origin_program: RoleProgram<0> = project(&program);
                let rv_id = cluster
                    .add_rendezvous_from_config(
                        Config::new(tap_buf, slab).with_universe(LowLabelUniverse),
                        transport.clone(),
                    )
                    .expect("register rendezvous");

                let err =
                    match cluster.enter(rv_id, SessionId::new(201), &origin_program, NoBinding) {
                        Ok(_) => panic!("custom label universe must reject high logical label"),
                        Err(err) => err,
                    };

                assert!(matches!(
                    err,
                    hibana::substrate::AttachError::Control(
                        hibana::substrate::CpError::LabelOutOfUniverse {
                            max: 127,
                            actual: 200
                        }
                    )
                ));
                assert!(transport_queue_is_empty(&transport));
            },
        );
    });
}

#[test]
fn cursor_send_and_recv_manual_wire_control_token() {
    with_fixture(|clock, tap_buf, slab| {
        let transport = TestTransport::default();
        with_tls_ref(
            &SESSION_SLOT,
            |ptr| unsafe {
                ptr.write(SessionKit::new(clock));
            },
            |cluster| {
                let program = g::send::<
                    Role<0>,
                    Role<1>,
                    Msg<
                        { MANUAL_WIRE_CONTROL_LOGICAL },
                        GenericCapToken<ManualWireControl>,
                        ManualWireControl,
                    >,
                    0,
                >();
                let origin_program: RoleProgram<0> = project(&program);
                let target_program: RoleProgram<1> = project(&program);
                let rv_id = cluster
                    .add_rendezvous_from_config(Config::new(tap_buf, slab), transport.clone())
                    .expect("register rendezvous");

                let sid = SessionId::new(9);
                let mut origin_endpoint = cluster
                    .enter(rv_id, sid, &origin_program, NoBinding)
                    .expect("origin endpoint");
                let mut target_endpoint = cluster
                    .enter(rv_id, sid, &target_program, NoBinding)
                    .expect("target endpoint");

                let token = manual_wire_token(sid, hibana::substrate::ids::Lane::new(0), 1);

                let () = futures::executor::block_on(
                    origin_endpoint
                        .flow::<Msg<
                            { MANUAL_WIRE_CONTROL_LOGICAL },
                            GenericCapToken<ManualWireControl>,
                            ManualWireControl,
                        >>()
                        .expect("wire control flow")
                        .send(&token),
                )
                .expect("explicit wire control token send succeeds");

                let received = futures::executor::block_on(target_endpoint.recv::<Msg<
                    { MANUAL_WIRE_CONTROL_LOGICAL },
                    GenericCapToken<ManualWireControl>,
                    ManualWireControl,
                >>())
                .expect("recv succeeds");

                assert_eq!(
                    received.decode_handle().expect("decode handle"),
                    (sid.raw(), 0)
                );
                assert_eq!(received.into_bytes(), token.into_bytes());
                assert!(transport_queue_is_empty(&transport));
            },
        );
    });
}

#[test]
fn deterministic_recv_rejects_control_data_kind_mismatch() {
    with_fixture(|clock, tap_buf, slab| {
        let transport = TestTransport::default();
        with_tls_ref(
            &SESSION_SLOT,
            |ptr| unsafe {
                ptr.write(SessionKit::new(clock));
            },
            |cluster| {
                let program = g::send::<
                    Role<0>,
                    Role<1>,
                    Msg<
                        { MANUAL_WIRE_CONTROL_LOGICAL },
                        GenericCapToken<ManualWireControl>,
                        ManualWireControl,
                    >,
                    0,
                >();
                let origin_program: RoleProgram<0> = project(&program);
                let target_program: RoleProgram<1> = project(&program);
                let rv_id = cluster
                    .add_rendezvous_from_config(Config::new(tap_buf, slab), transport.clone())
                    .expect("register rendezvous");

                let sid = SessionId::new(91);
                let mut origin_endpoint = cluster
                    .enter(rv_id, sid, &origin_program, NoBinding)
                    .expect("origin endpoint");
                let mut target_endpoint = cluster
                    .enter(rv_id, sid, &target_program, NoBinding)
                    .expect("target endpoint");

                let token = manual_wire_token(sid, hibana::substrate::ids::Lane::new(0), 1);
                futures::executor::block_on(
                    origin_endpoint
                        .flow::<Msg<
                            { MANUAL_WIRE_CONTROL_LOGICAL },
                            GenericCapToken<ManualWireControl>,
                            ManualWireControl,
                        >>()
                        .expect("wire control flow")
                        .send(&token),
                )
                .expect("explicit wire control token send succeeds");

                let err = match futures::executor::block_on(
                    target_endpoint
                        .recv::<Msg<{ MANUAL_WIRE_CONTROL_LOGICAL }, [u8; MANUAL_TOKEN_LEN]>>(),
                ) {
                    Ok(_) => panic!("deterministic recv must reject control as data"),
                    Err(err) => err,
                };
                assert!(matches!(err, hibana::RecvError::PhaseInvariant));
            },
        );
    });

    with_fixture(|clock, tap_buf, slab| {
        let transport = TestTransport::default();
        with_tls_ref(
            &SESSION_SLOT,
            |ptr| unsafe {
                ptr.write(SessionKit::new(clock));
            },
            |cluster| {
                let program = g::send::<
                    Role<0>,
                    Role<1>,
                    Msg<{ MANUAL_WIRE_CONTROL_LOGICAL }, [u8; MANUAL_TOKEN_LEN]>,
                    0,
                >();
                let origin_program: RoleProgram<0> = project(&program);
                let target_program: RoleProgram<1> = project(&program);
                let rv_id = cluster
                    .add_rendezvous_from_config(Config::new(tap_buf, slab), transport.clone())
                    .expect("register rendezvous");

                let sid = SessionId::new(92);
                let mut origin_endpoint = cluster
                    .enter(rv_id, sid, &origin_program, NoBinding)
                    .expect("origin endpoint");
                let mut target_endpoint = cluster
                    .enter(rv_id, sid, &target_program, NoBinding)
                    .expect("target endpoint");

                let token_bytes =
                    manual_wire_token(sid, hibana::substrate::ids::Lane::new(0), 1).into_bytes();
                futures::executor::block_on(
                    origin_endpoint
                        .flow::<Msg<{ MANUAL_WIRE_CONTROL_LOGICAL }, [u8; MANUAL_TOKEN_LEN]>>()
                        .expect("data flow")
                        .send(&token_bytes),
                )
                .expect("data send succeeds");

                let err = match futures::executor::block_on(target_endpoint.recv::<Msg<
                    { MANUAL_WIRE_CONTROL_LOGICAL },
                    GenericCapToken<ManualWireControl>,
                    ManualWireControl,
                >>()) {
                    Ok(_) => panic!("deterministic recv must reject data as control"),
                    Err(err) => err,
                };
                assert!(matches!(err, hibana::RecvError::PhaseInvariant));
            },
        );
    });
}

#[test]
fn manual_wire_control_send_dispatches_exactly_one_abort_ack() {
    with_fixture(|clock, tap_buf, slab| {
        let transport = TestTransport::default();
        let tap_ptr = tap_buf as *mut _;
        with_tls_ref(
            &SESSION_SLOT,
            |ptr| unsafe {
                ptr.write(SessionKit::new(clock));
            },
            |cluster| {
                let program = g::send::<
                    Role<0>,
                    Role<1>,
                    Msg<
                        { MANUAL_WIRE_ABORT_ACK_LOGICAL },
                        GenericCapToken<ManualWireAbortAckControl>,
                        ManualWireAbortAckControl,
                    >,
                    0,
                >();
                let origin_program: RoleProgram<0> = project(&program);
                let target_program: RoleProgram<1> = project(&program);
                let rv_id = cluster
                    .add_rendezvous_from_config(
                        Config::new(unsafe { &mut *tap_ptr }, slab),
                        transport.clone(),
                    )
                    .expect("register rendezvous");

                let sid = SessionId::new(10);
                let mut origin_endpoint = cluster
                    .enter(rv_id, sid, &origin_program, NoBinding)
                    .expect("origin endpoint");
                let mut target_endpoint = cluster
                    .enter(rv_id, sid, &target_program, NoBinding)
                    .expect("target endpoint");

                let token =
                    manual_wire_abort_ack_token(sid, hibana::substrate::ids::Lane::new(0), 1, 0, 0);

                let () = futures::executor::block_on(
                    origin_endpoint
                        .flow::<Msg<
                            { MANUAL_WIRE_ABORT_ACK_LOGICAL },
                            GenericCapToken<ManualWireAbortAckControl>,
                            ManualWireAbortAckControl,
                        >>()
                        .expect("wire abort-ack flow")
                        .send(&token),
                )
                .expect("explicit wire abort-ack send succeeds");

                let received = futures::executor::block_on(target_endpoint.recv::<Msg<
                    { MANUAL_WIRE_ABORT_ACK_LOGICAL },
                    GenericCapToken<ManualWireAbortAckControl>,
                    ManualWireAbortAckControl,
                >>())
                .expect("recv succeeds");
                assert_eq!(received.into_bytes(), token.into_bytes());
                assert!(transport_queue_is_empty(&transport));
            },
        );
        let abort_ack_events = unsafe { &*tap_ptr }
            .iter()
            .filter(|event| event.id == ABORT_ACK_ID && event.arg0 == 10)
            .count();
        assert_eq!(
            abort_ack_events, 1,
            "explicit wire control send must execute exactly one abort-ack operation",
        );
    });
}

#[test]
fn manual_wire_one_shot_control_send_rejects_before_transport() {
    with_fixture(|clock, tap_buf, slab| {
        let transport = TestTransport::default();
        let tap_ptr = tap_buf as *mut _;
        with_tls_ref(
            &SESSION_SLOT,
            |ptr| unsafe {
                ptr.write(SessionKit::new(clock));
            },
            |cluster| {
                let program = g::send::<
                    Role<0>,
                    Role<1>,
                    Msg<
                        { MANUAL_WIRE_ONE_SHOT_ABORT_ACK_LOGICAL },
                        GenericCapToken<ManualWireOneShotAbortAckControl>,
                        ManualWireOneShotAbortAckControl,
                    >,
                    0,
                >();
                let origin_program: RoleProgram<0> = project(&program);
                let target_program: RoleProgram<1> = project(&program);
                let rv_id = cluster
                    .add_rendezvous_from_config(
                        Config::new(unsafe { &mut *tap_ptr }, slab),
                        transport.clone(),
                    )
                    .expect("register rendezvous");

                let sid = SessionId::new(18);
                let mut origin_endpoint = cluster
                    .enter(rv_id, sid, &origin_program, NoBinding)
                    .expect("origin endpoint");
                let target_endpoint = cluster
                    .enter(rv_id, sid, &target_program, NoBinding)
                    .expect("target endpoint");
                core::hint::black_box(&target_endpoint);

                let token = manual_wire_one_shot_abort_ack_token(
                    sid,
                    hibana::substrate::ids::Lane::new(0),
                    1,
                    0,
                    0,
                );

                let err = futures::executor::block_on(
                    origin_endpoint
                        .flow::<Msg<
                            { MANUAL_WIRE_ONE_SHOT_ABORT_ACK_LOGICAL },
                            GenericCapToken<ManualWireOneShotAbortAckControl>,
                            ManualWireOneShotAbortAckControl,
                        >>()
                        .expect("wire one-shot abort-ack flow")
                        .send(&token),
                )
                .expect_err("unclaimed one-shot manual wire token must fail before transport");

                assert!(matches!(err, hibana::SendError::PhaseInvariant));
                assert!(transport_queue_is_empty(&transport));
            },
        );
        assert!(
            !unsafe { &*tap_ptr }
                .iter()
                .any(|event| event.id == ABORT_ACK_ID && event.arg0 == 18),
            "unclaimed one-shot manual wire token must not execute abort-ack",
        );
    });
}

#[test]
fn manual_wire_control_send_rejects_scope_mismatch_before_transport() {
    with_fixture(|clock, tap_buf, slab| {
        let transport = TestTransport::default();
        let tap_ptr = tap_buf as *mut _;
        with_tls_ref(
            &SESSION_SLOT,
            |ptr| unsafe {
                ptr.write(SessionKit::new(clock));
            },
            |cluster| {
                let program = g::send::<
                    Role<0>,
                    Role<1>,
                    Msg<
                        { MANUAL_WIRE_ABORT_ACK_LOGICAL },
                        GenericCapToken<ManualWireAbortAckControl>,
                        ManualWireAbortAckControl,
                    >,
                    0,
                >();
                let origin_program: RoleProgram<0> = project(&program);
                let target_program: RoleProgram<1> = project(&program);
                let rv_id = cluster
                    .add_rendezvous_from_config(
                        Config::new(unsafe { &mut *tap_ptr }, slab),
                        transport.clone(),
                    )
                    .expect("register rendezvous");

                let sid = SessionId::new(11);
                let mut origin_endpoint = cluster
                    .enter(rv_id, sid, &origin_program, NoBinding)
                    .expect("origin endpoint");
                let target_endpoint = cluster
                    .enter(rv_id, sid, &target_program, NoBinding)
                    .expect("target endpoint");
                core::hint::black_box(&target_endpoint);

                let mismatched =
                    manual_wire_abort_ack_token(sid, hibana::substrate::ids::Lane::new(0), 1, 1, 0);

                let err = futures::executor::block_on(
                    origin_endpoint
                        .flow::<Msg<
                            { MANUAL_WIRE_ABORT_ACK_LOGICAL },
                            GenericCapToken<ManualWireAbortAckControl>,
                            ManualWireAbortAckControl,
                        >>()
                        .expect("wire abort-ack flow")
                        .send(&mismatched),
                )
                .expect_err("descriptor/header mismatch must fail before transport");

                assert!(matches!(err, hibana::SendError::PhaseInvariant));
                assert!(transport_queue_is_empty(&transport));
            },
        );
        assert!(
            !unsafe { &*tap_ptr }
                .iter()
                .any(|event| event.id == ABORT_ACK_ID && event.arg0 == 11),
            "rejected explicit control send must not execute abort-ack",
        );
    });
}

#[test]
fn manual_wire_control_send_rejects_session_binding_before_transport() {
    let sid = SessionId::new(12);
    let token = manual_wire_abort_ack_token(
        SessionId::new(13),
        hibana::substrate::ids::Lane::new(0),
        1,
        0,
        0,
    );
    assert_manual_wire_abort_ack_send_rejected(token, sid);
}

#[test]
fn manual_wire_control_send_rejects_lane_binding_before_transport() {
    let sid = SessionId::new(14);
    let token = manual_wire_abort_ack_token(sid, hibana::substrate::ids::Lane::new(1), 1, 0, 0);
    assert_manual_wire_abort_ack_send_rejected(token, sid);
}

#[test]
fn manual_wire_control_send_rejects_role_binding_before_transport() {
    let sid = SessionId::new(15);
    let token = manual_wire_abort_ack_token(sid, hibana::substrate::ids::Lane::new(0), 0, 0, 0);
    assert_manual_wire_abort_ack_send_rejected(token, sid);
}

#[test]
fn manual_wire_control_send_rejects_handle_mismatch_before_transport() {
    let sid = SessionId::new(16);
    let token = manual_wire_abort_ack_token_with_handle(
        sid,
        hibana::substrate::ids::Lane::new(0),
        1,
        0,
        0,
        sid.raw(),
        1,
    );
    assert_manual_wire_abort_ack_send_rejected(token, sid);
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
                let program = g::send::<Role<0>, Role<1>, Msg<1, u32>, 0>();
                let origin_program: RoleProgram<0> = project(&program);
                let target_program: RoleProgram<1> = project(&program);
                let rv_id = cluster
                    .add_rendezvous_from_config(Config::new(tap_buf, slab), transport)
                    .expect("register rendezvous");

                let sid = SessionId::new(3);
                let mut origin_endpoint = cluster
                    .enter(rv_id, sid, &origin_program, NoBinding)
                    .expect("origin endpoint");
                let mut target_endpoint = cluster
                    .enter(rv_id, sid, &target_program, NoBinding)
                    .expect("target endpoint");

                let send = origin_endpoint
                    .flow::<Msg<1, u32>>()
                    .expect("send flow")
                    .send(&42);
                let recv = target_endpoint.recv::<Msg<1, u32>>();

                let endpoint_bytes = size_of::<hibana::Endpoint<'static, 0>>();
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
