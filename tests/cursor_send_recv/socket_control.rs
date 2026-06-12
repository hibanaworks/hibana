use super::*;

use core::cell::UnsafeCell;
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{Arc, Mutex},
    time::Duration,
};

use hibana::integration::runtime::{DefaultLabelUniverse, TapEvent};

const SOCKET_FIXTURE_SLAB_CAPACITY: usize = 1_048_576;
const SOCKET_FRAME_HEADER_LEN: usize = 10;
const SOCKET_FRAME_PAYLOAD_CAPACITY: usize = 512;
const CONTROL_TOKEN_WIRE_LEN: usize = 56;
pub(super) const TOPOLOGY_BEGIN_LABEL: u8 = 90;
const TOPOLOGY_ACK_LABEL: u8 = 91;
const TOPOLOGY_COMMIT_LABEL: u8 = 92;
const TOPOLOGY_DUP_BEGIN_LABEL: u8 = 93;
const TOPOLOGY_PHASE_ACK_LABEL: u8 = 94;
const TAP_ENDPOINT_CONTROL: u16 = 0x0204;
pub(super) const TAP_TOPOLOGY_BEGIN: u16 = 0x0208;
const TAP_TOPOLOGY_ACK: u16 = 0x0209;
const TAP_TOPOLOGY_COMMIT: u16 = 0x020A;

pub(super) type SocketKitStorage =
    SessionKitStorage<'static, TcpLoopbackTransport, DefaultLabelUniverse, CounterClock, 2>;

std::thread_local! {
    pub(super) static SOCKET_SESSION_SLOT: UnsafeCell<SocketKitStorage> =
        const { UnsafeCell::new(SessionKitStorage::uninit()) };
    static SOCKET_TAP0: UnsafeCell<[TapEvent; runtime_support::RING_EVENTS]> =
        const { UnsafeCell::new([TapEvent::zero(); runtime_support::RING_EVENTS]) };
    static SOCKET_TAP1: UnsafeCell<[TapEvent; runtime_support::RING_EVENTS]> =
        const { UnsafeCell::new([TapEvent::zero(); runtime_support::RING_EVENTS]) };
    static SOCKET_SLAB0: UnsafeCell<[u8; SOCKET_FIXTURE_SLAB_CAPACITY]> =
        const { UnsafeCell::new([0u8; SOCKET_FIXTURE_SLAB_CAPACITY]) };
    static SOCKET_SLAB1: UnsafeCell<[u8; SOCKET_FIXTURE_SLAB_CAPACITY]> =
        const { UnsafeCell::new([0u8; SOCKET_FIXTURE_SLAB_CAPACITY]) };
}

#[derive(Debug)]
pub(super) enum SocketTransportError {
    Io,
    Malformed,
}

impl From<SocketTransportError> for hibana::integration::transport::TransportError {
    fn from(err: SocketTransportError) -> Self {
        match err {
            SocketTransportError::Io | SocketTransportError::Malformed => Self::Failed,
        }
    }
}

struct SocketControlStats {
    sent_frames: usize,
    received_frames: usize,
    last_payload_len: usize,
}

impl SocketControlStats {
    fn new() -> Self {
        Self {
            sent_frames: 0,
            received_frames: 0,
            last_payload_len: 0,
        }
    }
}

struct SocketShared {
    role0: Mutex<TcpStream>,
    role1: Mutex<TcpStream>,
    stats: Mutex<SocketControlStats>,
}

#[derive(Clone)]
pub(super) struct TcpLoopbackTransport {
    shared: Arc<SocketShared>,
}

pub(super) struct SocketTx {
    local_role: u8,
    session_id: SessionId,
}

struct SocketFrame {
    session_id: SessionId,
    lane: u8,
    source_role: u8,
    target_role: u8,
    frame_label: u8,
    len: usize,
    payload: [u8; SOCKET_FRAME_PAYLOAD_CAPACITY],
}

pub(super) struct SocketRx {
    local_role: u8,
    current: Option<SocketFrame>,
    requeued: Option<SocketFrame>,
}

impl TcpLoopbackTransport {
    pub(super) fn new() -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind tcp loopback listener");
        let addr = listener.local_addr().expect("tcp loopback listener addr");
        let role0 = TcpStream::connect(addr).expect("connect tcp loopback role0");
        let (role1, _) = listener.accept().expect("accept tcp loopback role1");
        role0
            .set_nodelay(true)
            .expect("disable role0 nagle for fixture");
        role1
            .set_nodelay(true)
            .expect("disable role1 nagle for fixture");
        role0
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("role0 read timeout");
        role1
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("role1 read timeout");
        Self {
            shared: Arc::new(SocketShared {
                role0: Mutex::new(role0),
                role1: Mutex::new(role1),
                stats: Mutex::new(SocketControlStats::new()),
            }),
        }
    }

    fn stream_for(&self, role: u8) -> Result<&Mutex<TcpStream>, SocketTransportError> {
        match role {
            0 => Ok(&self.shared.role0),
            1 => Ok(&self.shared.role1),
            _ => Err(SocketTransportError::Malformed),
        }
    }

    pub(super) fn sent_frames(&self) -> usize {
        self.shared
            .stats
            .lock()
            .expect("socket fixture stats lock")
            .sent_frames
    }

    pub(super) fn received_frames(&self) -> usize {
        self.shared
            .stats
            .lock()
            .expect("socket fixture stats lock")
            .received_frames
    }

    fn last_payload_len(&self) -> usize {
        self.shared
            .stats
            .lock()
            .expect("socket fixture stats lock")
            .last_payload_len
    }
}

impl SocketFrame {
    fn encode_header(
        session_id: SessionId,
        lane: u8,
        source_role: u8,
        target_role: u8,
        frame_label: u8,
        payload_len: usize,
    ) -> Result<[u8; SOCKET_FRAME_HEADER_LEN], SocketTransportError> {
        let payload_len =
            u16::try_from(payload_len).map_err(|_| SocketTransportError::Malformed)?;
        let mut header = [0u8; SOCKET_FRAME_HEADER_LEN];
        header[0..4].copy_from_slice(&session_id.raw().to_be_bytes());
        header[4] = lane;
        header[5] = source_role;
        header[6] = target_role;
        header[7] = frame_label;
        header[8..10].copy_from_slice(&payload_len.to_be_bytes());
        Ok(header)
    }

    fn read_from(stream: &mut TcpStream) -> Result<Self, SocketTransportError> {
        let mut header = [0u8; SOCKET_FRAME_HEADER_LEN];
        stream
            .read_exact(&mut header)
            .map_err(|_| SocketTransportError::Io)?;
        let len = u16::from_be_bytes([header[8], header[9]]) as usize;
        if len > SOCKET_FRAME_PAYLOAD_CAPACITY {
            return Err(SocketTransportError::Malformed);
        }
        let mut payload = [0u8; SOCKET_FRAME_PAYLOAD_CAPACITY];
        stream
            .read_exact(&mut payload[..len])
            .map_err(|_| SocketTransportError::Io)?;
        Ok(Self {
            session_id: SessionId::new(u32::from_be_bytes([
                header[0], header[1], header[2], header[3],
            ])),
            lane: header[4],
            source_role: header[5],
            target_role: header[6],
            frame_label: header[7],
            len,
            payload,
        })
    }

    fn payload(&self) -> &[u8] {
        &self.payload[..self.len]
    }
}

impl Transport for TcpLoopbackTransport {
    type Error = SocketTransportError;
    type Tx<'a>
        = SocketTx
    where
        Self: 'a;
    type Rx<'a>
        = SocketRx
    where
        Self: 'a;

    fn open<'a>(
        &'a self,
        port: hibana::integration::transport::PortOpen,
    ) -> (Self::Tx<'a>, Self::Rx<'a>) {
        (
            SocketTx {
                local_role: port.local_role(),
                session_id: port.session_id(),
            },
            SocketRx {
                local_role: port.local_role(),
                current: None,
                requeued: None,
            },
        )
    }

    fn poll_send<'a, 'f>(
        &self,
        tx: &'a mut Self::Tx<'a>,
        outgoing: Outgoing<'f>,
        _context: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>>
    where
        'a: 'f,
    {
        let payload = outgoing.payload().as_bytes();
        let header = SocketFrame::encode_header(
            tx.session_id,
            outgoing.lane(),
            tx.local_role,
            outgoing.peer(),
            outgoing.frame_label().raw(),
            payload.len(),
        )?;
        let stream = self.stream_for(tx.local_role)?;
        let mut stream = stream.lock().map_err(|_| SocketTransportError::Io)?;
        stream
            .write_all(&header)
            .and_then(|()| stream.write_all(payload))
            .and_then(|()| stream.flush())
            .map_err(|_| SocketTransportError::Io)?;
        {
            let mut stats = self
                .shared
                .stats
                .lock()
                .map_err(|_| SocketTransportError::Io)?;
            stats.sent_frames += 1;
            stats.last_payload_len = payload.len();
        }
        Poll::Ready(Ok(()))
    }

    fn cancel_send<'a>(&self, _tx: &'a mut Self::Tx<'a>) {}

    fn poll_recv<'a>(
        &'a self,
        rx: &'a mut Self::Rx<'a>,
        _context: &mut Context<'_>,
    ) -> Poll<Result<ReceivedFrame<'a>, Self::Error>> {
        if rx.current.is_some() {
            rx.current = None;
        }
        let frame = if let Some(frame) = rx.requeued.take() {
            frame
        } else {
            let stream = self.stream_for(rx.local_role)?;
            let mut stream = stream.lock().map_err(|_| SocketTransportError::Io)?;
            SocketFrame::read_from(&mut stream)?
        };
        let header = hibana::integration::transport::FrameHeader::new(
            frame.session_id,
            frame.lane,
            frame.source_role,
            frame.target_role,
            hibana::integration::transport::FrameLabel::new(frame.frame_label),
        );
        rx.current = Some(frame);
        let frame = rx.current.as_ref().expect("socket frame staged");
        let bytes: &'a [u8] = unsafe { &*(frame.payload() as *const [u8]) };
        self.shared
            .stats
            .lock()
            .map_err(|_| SocketTransportError::Io)?
            .received_frames += 1;
        Poll::Ready(Ok(ReceivedFrame::framed(header, Payload::new(bytes))))
    }

    fn requeue<'a>(&self, rx: &mut Self::Rx<'a>) -> Result<(), Self::Error> {
        if let Some(frame) = rx.current.take() {
            rx.requeued = Some(frame);
        }
        Ok(())
    }
}

pub(super) fn with_socket_fixture_pair<R>(
    f: impl FnOnce(
        &'static mut [TapEvent; runtime_support::RING_EVENTS],
        &'static mut [u8],
        &'static mut [TapEvent; runtime_support::RING_EVENTS],
        &'static mut [u8],
    ) -> R,
) -> R {
    SOCKET_TAP0.with(|tap0| {
        SOCKET_TAP1.with(|tap1| {
            SOCKET_SLAB0.with(|slab0| {
                SOCKET_SLAB1.with(|slab1| unsafe {
                    let tap0 = &mut *tap0.get();
                    let tap1 = &mut *tap1.get();
                    let slab0 = &mut *slab0.get();
                    let slab1 = &mut *slab1.get();
                    tap0.fill(TapEvent::zero());
                    tap1.fill(TapEvent::zero());
                    slab0.fill(0);
                    slab1.fill(0);
                    f(
                        &mut *(tap0 as *mut [TapEvent; runtime_support::RING_EVENTS]),
                        &mut *(slab0.as_mut_slice() as *mut [u8]),
                        &mut *(tap1 as *mut [TapEvent; runtime_support::RING_EVENTS]),
                        &mut *(slab1.as_mut_slice() as *mut [u8]),
                    )
                })
            })
        })
    })
}

fn tap_count(tap: &[TapEvent], id: u16) -> usize {
    tap.iter().filter(|event| event.id == id).count()
}

pub(super) fn tap_pair_count(left: &[TapEvent], right: &[TapEvent], id: u16) -> usize {
    tap_count(left, id) + tap_count(right, id)
}

#[test]
fn public_topology_controlmsg_three_phase_roundtrips_over_tcp_loopback() {
    with_socket_fixture_pair(|tap0, slab0, tap1, slab1| {
        let tap0_ptr = tap0.as_mut_ptr();
        let tap1_ptr = tap1.as_mut_ptr();
        let transport = TcpLoopbackTransport::new();
        with_resident_tls_ref(&SOCKET_SESSION_SLOT, |cluster| {
            let program = g::seq(
                g::send::<0, 1, g::ControlMsg<TOPOLOGY_BEGIN_LABEL, g::control::TopologyBegin>>(),
                g::seq(
                    g::send::<1, 0, g::ControlMsg<TOPOLOGY_ACK_LABEL, g::control::TopologyAck>>(),
                    g::send::<0, 1, g::ControlMsg<TOPOLOGY_COMMIT_LABEL, g::control::TopologyCommit>>(
                    ),
                ),
            );
            let origin_program: RoleProgram<0> = project(&program);
            let target_program: RoleProgram<1> = project(&program);
            let rv0 = cluster
                .rendezvous(
                    Config::<DefaultLabelUniverse, _>::from_resources(
                        (tap0, slab0),
                        CounterClock::new(),
                    ),
                    transport.clone(),
                )
                .expect("register source rendezvous");
            let rv1 = cluster
                .rendezvous(
                    Config::<DefaultLabelUniverse, _>::from_resources(
                        (tap1, slab1),
                        CounterClock::new(),
                    ),
                    transport.clone(),
                )
                .expect("register destination rendezvous");

            let sid = SessionId::new(90);
            let mut origin_endpoint = rv0
                .session(sid)
                .role(&origin_program)
                .enter()
                .expect("source endpoint");
            let mut target_endpoint = rv1
                .session(sid)
                .role(&target_program)
                .enter()
                .expect("destination endpoint");

            futures::executor::block_on(
                origin_endpoint
                    .flow::<g::ControlMsg<TOPOLOGY_BEGIN_LABEL, g::control::TopologyBegin>>()
                    .expect("topology begin flow")
                    .send(&()),
            )
            .expect("topology begin send over tcp loopback");
            assert_eq!(transport.sent_frames(), 1);
            assert_eq!(transport.last_payload_len(), CONTROL_TOKEN_WIRE_LEN);
            let received = futures::executor::block_on(
                target_endpoint
                    .recv::<g::ControlMsg<TOPOLOGY_BEGIN_LABEL, g::control::TopologyBegin>>(),
            );
            assert!(received.is_ok(), "topology begin recv failed: {received:?}");
            assert_eq!(transport.received_frames(), 1);

            futures::executor::block_on(
                target_endpoint
                    .flow::<g::ControlMsg<TOPOLOGY_ACK_LABEL, g::control::TopologyAck>>()
                    .expect("topology ack flow")
                    .send(&()),
            )
            .expect("topology ack send over tcp loopback");
            assert_eq!(transport.sent_frames(), 2);
            assert_eq!(transport.last_payload_len(), CONTROL_TOKEN_WIRE_LEN);
            let received = futures::executor::block_on(
                origin_endpoint
                    .recv::<g::ControlMsg<TOPOLOGY_ACK_LABEL, g::control::TopologyAck>>(),
            );
            assert!(received.is_ok(), "topology ack recv failed: {received:?}");
            assert_eq!(transport.received_frames(), 2);

            futures::executor::block_on(
                origin_endpoint
                    .flow::<g::ControlMsg<TOPOLOGY_COMMIT_LABEL, g::control::TopologyCommit>>()
                    .expect("topology commit flow")
                    .send(&()),
            )
            .expect("topology commit send over tcp loopback");
            assert_eq!(transport.sent_frames(), 3);
            assert_eq!(transport.last_payload_len(), CONTROL_TOKEN_WIRE_LEN);
            let received = futures::executor::block_on(
                target_endpoint
                    .recv::<g::ControlMsg<TOPOLOGY_COMMIT_LABEL, g::control::TopologyCommit>>(),
            );
            assert!(
                received.is_ok(),
                "topology commit recv failed: {received:?}"
            );
            assert_eq!(transport.received_frames(), 3);

            let tap0 =
                unsafe { core::slice::from_raw_parts(tap0_ptr, runtime_support::RING_EVENTS) };
            let tap1 =
                unsafe { core::slice::from_raw_parts(tap1_ptr, runtime_support::RING_EVENTS) };
            assert!(
                tap_pair_count(tap0, tap1, TAP_ENDPOINT_CONTROL) >= 6,
                "public topology ControlMsg must emit endpoint-control TAP on send and recv"
            );
            assert_eq!(tap_pair_count(tap0, tap1, TAP_TOPOLOGY_BEGIN), 1);
            assert_eq!(tap_pair_count(tap0, tap1, TAP_TOPOLOGY_ACK), 1);
            assert_eq!(tap_pair_count(tap0, tap1, TAP_TOPOLOGY_COMMIT), 1);
            assert!(
                transport.last_payload_len() == CONTROL_TOKEN_WIRE_LEN,
                "unit public ControlMsg must mint exact wire control token bytes before crossing TCP"
            );
        });
    });
}

#[test]
fn public_topology_controlmsg_ack_before_begin_fails_closed_over_tcp_loopback() {
    with_socket_fixture_pair(|tap0, slab0, tap1, slab1| {
        let tap0_ptr = tap0.as_mut_ptr();
        let tap1_ptr = tap1.as_mut_ptr();
        let transport = TcpLoopbackTransport::new();
        with_resident_tls_ref(&SOCKET_SESSION_SLOT, |cluster| {
            let program =
                g::send::<1, 0, g::ControlMsg<TOPOLOGY_ACK_LABEL, g::control::TopologyAck>>();
            let origin_program: RoleProgram<0> = project(&program);
            let target_program: RoleProgram<1> = project(&program);
            let rv0 = cluster
                .rendezvous(
                    Config::<DefaultLabelUniverse, _>::from_resources(
                        (tap0, slab0),
                        CounterClock::new(),
                    ),
                    transport.clone(),
                )
                .expect("register source rendezvous");
            let rv1 = cluster
                .rendezvous(
                    Config::<DefaultLabelUniverse, _>::from_resources(
                        (tap1, slab1),
                        CounterClock::new(),
                    ),
                    transport.clone(),
                )
                .expect("register destination rendezvous");

            let sid = SessionId::new(91);
            let origin_endpoint = rv0
                .session(sid)
                .role(&origin_program)
                .enter()
                .expect("source endpoint");
            let mut target_endpoint = rv1
                .session(sid)
                .role(&target_program)
                .enter()
                .expect("destination endpoint");

            let err = futures::executor::block_on(
                target_endpoint
                    .flow::<g::ControlMsg<TOPOLOGY_ACK_LABEL, g::control::TopologyAck>>()
                    .expect("topology ack flow")
                    .send(&()),
            )
            .expect_err("topology ack before begin must fail closed");
            assert!(
                format!("{err:?}").contains("PhaseInvariant"),
                "topology ack before begin must fail as a phase invariant, got {err:?}"
            );
            assert_eq!(transport.sent_frames(), 0);
            assert_eq!(transport.received_frames(), 0);
            let tap0 =
                unsafe { core::slice::from_raw_parts(tap0_ptr, runtime_support::RING_EVENTS) };
            let tap1 =
                unsafe { core::slice::from_raw_parts(tap1_ptr, runtime_support::RING_EVENTS) };
            assert_eq!(tap_pair_count(tap0, tap1, TAP_ENDPOINT_CONTROL), 0);
            assert_eq!(tap_pair_count(tap0, tap1, TAP_TOPOLOGY_ACK), 0);
            drop(origin_endpoint);
        });
    });
}

#[test]
fn public_topology_controlmsg_commit_before_ack_fails_closed_over_tcp_loopback() {
    with_socket_fixture_pair(|tap0, slab0, tap1, slab1| {
        let tap0_ptr = tap0.as_mut_ptr();
        let tap1_ptr = tap1.as_mut_ptr();
        let transport = TcpLoopbackTransport::new();
        with_resident_tls_ref(&SOCKET_SESSION_SLOT, |cluster| {
            let program = g::seq(
                g::send::<0, 1, g::ControlMsg<TOPOLOGY_BEGIN_LABEL, g::control::TopologyBegin>>(),
                g::send::<0, 1, g::ControlMsg<TOPOLOGY_COMMIT_LABEL, g::control::TopologyCommit>>(),
            );
            let origin_program: RoleProgram<0> = project(&program);
            let target_program: RoleProgram<1> = project(&program);
            let rv0 = cluster
                .rendezvous(
                    Config::<DefaultLabelUniverse, _>::from_resources(
                        (tap0, slab0),
                        CounterClock::new(),
                    ),
                    transport.clone(),
                )
                .expect("register source rendezvous");
            let rv1 = cluster
                .rendezvous(
                    Config::<DefaultLabelUniverse, _>::from_resources(
                        (tap1, slab1),
                        CounterClock::new(),
                    ),
                    transport.clone(),
                )
                .expect("register destination rendezvous");

            let sid = SessionId::new(92);
            let mut origin_endpoint = rv0
                .session(sid)
                .role(&origin_program)
                .enter()
                .expect("source endpoint");
            let mut target_endpoint = rv1
                .session(sid)
                .role(&target_program)
                .enter()
                .expect("destination endpoint");

            futures::executor::block_on(
                origin_endpoint
                    .flow::<g::ControlMsg<TOPOLOGY_BEGIN_LABEL, g::control::TopologyBegin>>()
                    .expect("topology begin flow")
                    .send(&()),
            )
            .expect("topology begin send over tcp loopback");
            assert_eq!(transport.sent_frames(), 1);
            let received = futures::executor::block_on(
                target_endpoint
                    .recv::<g::ControlMsg<TOPOLOGY_BEGIN_LABEL, g::control::TopologyBegin>>(),
            );
            assert!(received.is_ok(), "topology begin recv failed: {received:?}");
            assert_eq!(transport.received_frames(), 1);

            let err = futures::executor::block_on(
                origin_endpoint
                    .flow::<g::ControlMsg<TOPOLOGY_COMMIT_LABEL, g::control::TopologyCommit>>()
                    .expect("topology commit flow")
                    .send(&()),
            )
            .expect_err("topology commit before ack must fail closed");
            assert!(
                format!("{err:?}").contains("PhaseInvariant"),
                "topology commit before ack must fail as a phase invariant, got {err:?}"
            );
            assert_eq!(transport.sent_frames(), 1);
            assert_eq!(transport.received_frames(), 1);
            let tap0 =
                unsafe { core::slice::from_raw_parts(tap0_ptr, runtime_support::RING_EVENTS) };
            let tap1 =
                unsafe { core::slice::from_raw_parts(tap1_ptr, runtime_support::RING_EVENTS) };
            assert!(
                tap_pair_count(tap0, tap1, TAP_ENDPOINT_CONTROL) >= 2,
                "successful begin send/recv must publish endpoint-control TAP"
            );
            assert_eq!(tap_pair_count(tap0, tap1, TAP_TOPOLOGY_BEGIN), 1);
            assert_eq!(tap_pair_count(tap0, tap1, TAP_TOPOLOGY_COMMIT), 0);
        });
    });
}

#[test]
fn public_topology_controlmsg_duplicate_begin_fails_closed_over_tcp_loopback() {
    with_socket_fixture_pair(|tap0, slab0, tap1, slab1| {
        let tap0_ptr = tap0.as_mut_ptr();
        let tap1_ptr = tap1.as_mut_ptr();
        let transport = TcpLoopbackTransport::new();
        with_resident_tls_ref(&SOCKET_SESSION_SLOT, |cluster| {
            let program = g::seq(
                g::send::<0, 1, g::ControlMsg<TOPOLOGY_BEGIN_LABEL, g::control::TopologyBegin>>(),
                g::send::<0, 1, g::ControlMsg<TOPOLOGY_DUP_BEGIN_LABEL, g::control::TopologyBegin>>(
                ),
            );
            let origin_program: RoleProgram<0> = project(&program);
            let target_program: RoleProgram<1> = project(&program);
            let rv0 = cluster
                .rendezvous(
                    Config::<DefaultLabelUniverse, _>::from_resources(
                        (tap0, slab0),
                        CounterClock::new(),
                    ),
                    transport.clone(),
                )
                .expect("register source rendezvous");
            let rv1 = cluster
                .rendezvous(
                    Config::<DefaultLabelUniverse, _>::from_resources(
                        (tap1, slab1),
                        CounterClock::new(),
                    ),
                    transport.clone(),
                )
                .expect("register destination rendezvous");

            let sid = SessionId::new(93);
            let mut origin_endpoint = rv0
                .session(sid)
                .role(&origin_program)
                .enter()
                .expect("source endpoint");
            let mut target_endpoint = rv1
                .session(sid)
                .role(&target_program)
                .enter()
                .expect("destination endpoint");

            futures::executor::block_on(
                origin_endpoint
                    .flow::<g::ControlMsg<TOPOLOGY_BEGIN_LABEL, g::control::TopologyBegin>>()
                    .expect("topology begin flow")
                    .send(&()),
            )
            .expect("topology begin send over tcp loopback");
            let received = futures::executor::block_on(
                target_endpoint
                    .recv::<g::ControlMsg<TOPOLOGY_BEGIN_LABEL, g::control::TopologyBegin>>(),
            );
            assert!(received.is_ok(), "topology begin recv failed: {received:?}");

            let err = futures::executor::block_on(
                origin_endpoint
                    .flow::<g::ControlMsg<TOPOLOGY_DUP_BEGIN_LABEL, g::control::TopologyBegin>>()
                    .expect("duplicate topology begin flow")
                    .send(&()),
            )
            .expect_err("duplicate topology begin must fail closed");
            assert!(
                format!("{err:?}").contains("PhaseInvariant"),
                "duplicate topology begin must fail as a phase invariant, got {err:?}"
            );
            assert_eq!(transport.sent_frames(), 1);
            assert_eq!(transport.received_frames(), 1);
            let tap0 =
                unsafe { core::slice::from_raw_parts(tap0_ptr, runtime_support::RING_EVENTS) };
            let tap1 =
                unsafe { core::slice::from_raw_parts(tap1_ptr, runtime_support::RING_EVENTS) };
            assert_eq!(tap_pair_count(tap0, tap1, TAP_TOPOLOGY_BEGIN), 1);
        });
    });
}

#[test]
fn public_topology_controlmsg_ack_from_source_fails_closed_over_tcp_loopback() {
    with_socket_fixture_pair(|tap0, slab0, tap1, slab1| {
        let tap0_ptr = tap0.as_mut_ptr();
        let tap1_ptr = tap1.as_mut_ptr();
        let transport = TcpLoopbackTransport::new();
        with_resident_tls_ref(&SOCKET_SESSION_SLOT, |cluster| {
            let program = g::seq(
                g::send::<0, 1, g::ControlMsg<TOPOLOGY_BEGIN_LABEL, g::control::TopologyBegin>>(),
                g::send::<0, 1, g::ControlMsg<TOPOLOGY_PHASE_ACK_LABEL, g::control::TopologyAck>>(),
            );
            let origin_program: RoleProgram<0> = project(&program);
            let target_program: RoleProgram<1> = project(&program);
            let rv0 = cluster
                .rendezvous(
                    Config::<DefaultLabelUniverse, _>::from_resources(
                        (tap0, slab0),
                        CounterClock::new(),
                    ),
                    transport.clone(),
                )
                .expect("register source rendezvous");
            let rv1 = cluster
                .rendezvous(
                    Config::<DefaultLabelUniverse, _>::from_resources(
                        (tap1, slab1),
                        CounterClock::new(),
                    ),
                    transport.clone(),
                )
                .expect("register destination rendezvous");

            let sid = SessionId::new(94);
            let mut origin_endpoint = rv0
                .session(sid)
                .role(&origin_program)
                .enter()
                .expect("source endpoint");
            let mut target_endpoint = rv1
                .session(sid)
                .role(&target_program)
                .enter()
                .expect("destination endpoint");

            futures::executor::block_on(
                origin_endpoint
                    .flow::<g::ControlMsg<TOPOLOGY_BEGIN_LABEL, g::control::TopologyBegin>>()
                    .expect("topology begin flow")
                    .send(&()),
            )
            .expect("topology begin send over tcp loopback");
            let received = futures::executor::block_on(
                target_endpoint
                    .recv::<g::ControlMsg<TOPOLOGY_BEGIN_LABEL, g::control::TopologyBegin>>(),
            );
            assert!(received.is_ok(), "topology begin recv failed: {received:?}");

            let err = futures::executor::block_on(
                origin_endpoint
                    .flow::<g::ControlMsg<TOPOLOGY_PHASE_ACK_LABEL, g::control::TopologyAck>>()
                    .expect("wrong-source topology ack flow")
                    .send(&()),
            )
            .expect_err("topology ack from source must fail closed");
            assert!(
                format!("{err:?}").contains("PhaseInvariant"),
                "topology ack from source must fail as a phase invariant, got {err:?}"
            );
            assert_eq!(transport.sent_frames(), 1);
            assert_eq!(transport.received_frames(), 1);
            let tap0 =
                unsafe { core::slice::from_raw_parts(tap0_ptr, runtime_support::RING_EVENTS) };
            let tap1 =
                unsafe { core::slice::from_raw_parts(tap1_ptr, runtime_support::RING_EVENTS) };
            assert_eq!(tap_pair_count(tap0, tap1, TAP_TOPOLOGY_BEGIN), 1);
            assert_eq!(tap_pair_count(tap0, tap1, TAP_TOPOLOGY_ACK), 0);
        });
    });
}
