//! Regression checks for the 9758c42 / 436d0ee security reports.
//! The final test deliberately violates carrier provenance. It records the
//! documented limit; it is not a claim of Byzantine agreement enforcement.
mod common;
#[path = "security_report_regressions/payload_replay.rs"]
mod payload_replay;
#[path = "security_report_regressions/received_payload.rs"]
mod received_payload;
#[path = "security_report_regressions/reentry_colors.rs"]
mod reentry_colors;
#[path = "security_report_regressions/rolled_routes.rs"]
mod rolled_routes;
#[path = "security_report_regressions/send_continuations.rs"]
mod send_continuations;

#[path = "../examples/support/in_process.rs"]
pub mod in_process;
use common::{TestRx, TestTransport, TestTx};
use core::{
    future::Future,
    pin::pin,
    task::{Context, Poll},
};
use hibana::{
    g::{self, Msg},
    runtime::{
        SessionKitStorage,
        ids::SessionId,
        program::{Projectable, RoleProgram, project},
        resolver::{DecisionArm, ResolverError, ResolverRef},
        transport::{Outgoing, PortOpen, ReceivedFrame, Transport, TransportError},
    },
};
use std::task::Waker;

#[derive(Clone, Debug)]
struct Captured {
    target: u8,
    lane: u8,
    label: u16,
    bytes: Vec<u8>,
}

struct RecordingCarrier<'a> {
    inner: TestTransport,
    frames: &'a core::cell::Cell<Vec<Captured>>,
}

impl Transport for RecordingCarrier<'_> {
    type Tx<'a>
        = <TestTransport as Transport>::Tx<'a>
    where
        Self: 'a;
    type Rx<'a>
        = <TestTransport as Transport>::Rx<'a>
    where
        Self: 'a;
    fn open<'a>(&'a self, port: PortOpen) -> (Self::Tx<'a>, Self::Rx<'a>) {
        self.inner.open(port)
    }
    fn poll_send<'f>(
        &self,
        tx: &mut TestTx,
        frame: Outgoing<'f>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), TransportError>> {
        let result = self.inner.poll_send(tx, frame, cx);
        if matches!(result, Poll::Ready(Ok(()))) {
            let mut frames = self.frames.take();
            frames.push(Captured {
                target: frame.target_role(),
                lane: frame.lane(),
                label: frame.frame_label().raw(),
                bytes: frame.payload().as_bytes().to_vec(),
            });
            self.frames.set(frames);
        }
        result
    }
    fn cancel_send(&self, tx: &mut TestTx) {
        self.inner.cancel_send(tx);
    }
    fn poll_recv<'a>(
        &'a self,
        rx: &'a mut TestRx<'_>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<ReceivedFrame<'a>, TransportError>> {
        self.inner.poll_recv(rx, cx)
    }
    fn requeue<'a>(&self, rx: &mut TestRx<'a>) -> Result<(), TransportError> {
        self.inner.requeue(rx)
    }
}

fn choice() -> impl Projectable {
    g::route(
        g::seq(
            g::send::<0, 1, Msg<31, u32>>(),
            g::send::<0, 2, Msg<32, u32>>(),
        ),
        g::seq(
            g::send::<0, 1, Msg<33, u32>>(),
            g::send::<0, 2, Msg<34, u32>>(),
        ),
    )
    .resolve::<900>()
}

fn ordered_lanes() -> impl Projectable {
    // `par` derives two distinct lanes. The preceding `seq` is a barrier for
    // both; the later lane must not be exposed merely because it exists.
    g::seq(
        g::send::<0, 1, Msg<10, u32>>(),
        g::par(
            g::send::<0, 1, Msg<11, u32>>(),
            g::send::<0, 1, Msg<20, u32>>(),
        ),
    )
}

fn choose(arm: &DecisionArm) -> Result<DecisionArm, ResolverError> {
    Ok(*arm)
}
static LEFT: DecisionArm = DecisionArm::Left;
static RIGHT: DecisionArm = DecisionArm::Right;

fn inject(carrier: &TestTransport, source: u8, frame: &Captured) {
    let mut tx = TestTx {
        session_id: SessionId::new(1),
        local_role: source,
        pending_role: None,
        pending_frame: None,
    };
    carrier.stage_send(&mut tx, frame.target, frame.lane, frame.label, &frame.bytes);
    assert!(matches!(
        carrier.poll_send_staged(&mut tx),
        Poll::Ready(Ok(()))
    ));
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn sequential_send_cannot_select_the_later_lane_first() {
    let frames = {
        let order = DecisionArm::Left;
        let frames = core::cell::Cell::new(Vec::new());
        let carrier = RecordingCarrier {
            inner: TestTransport::new(),
            frames: &frames,
        };
        let mut slab = [0; 128 * 1024];
        let mut storage = SessionKitStorage::<RecordingCarrier>::uninit();
        let rv = storage.init().rendezvous(&mut slab, carrier).unwrap();
        let program: RoleProgram<0> = project(&ordered_lanes());
        let mut origin = rv.enter(SessionId::new(1), &program).unwrap();
        (async {
            origin.send::<Msg<10, u32>>(&10).await.unwrap();
            match order {
                DecisionArm::Left => {
                    origin.send::<Msg<11, u32>>(&11).await.unwrap();
                    origin.send::<Msg<20, u32>>(&0xfeedbeef).await.unwrap();
                }
                DecisionArm::Right => {
                    origin.send::<Msg<20, u32>>(&0xfeedbeef).await.unwrap();
                    origin.send::<Msg<11, u32>>(&11).await.unwrap();
                }
            }
        })
        .await;
        frames.take()
    };
    assert_eq!(frames.len(), 3);
    assert_ne!(frames[0].lane, frames[2].lane);
    let carrier = TestTransport::new();
    let mut slab = [0; 128 * 1024];
    let mut storage = SessionKitStorage::<TestTransport>::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, carrier.clone())
        .unwrap();
    let program: RoleProgram<0> = project(&ordered_lanes());
    let mut origin = rv.enter(SessionId::new(1), &program).unwrap();
    assert!((origin.send::<Msg<20, u32>>(&0xfeedbeef)).await.is_err());
    assert!(
        carrier.queue_is_empty(),
        "no out-of-order frame may be published"
    );
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn sequential_receive_cannot_select_the_later_lane_first() {
    let frames = {
        let order = DecisionArm::Left;
        let frames = core::cell::Cell::new(Vec::new());
        let carrier = RecordingCarrier {
            inner: TestTransport::new(),
            frames: &frames,
        };
        let mut slab = [0; 128 * 1024];
        let mut storage = SessionKitStorage::<RecordingCarrier>::uninit();
        let rv = storage.init().rendezvous(&mut slab, carrier).unwrap();
        let program: RoleProgram<0> = project(&ordered_lanes());
        let mut origin = rv.enter(SessionId::new(1), &program).unwrap();
        (async {
            origin.send::<Msg<10, u32>>(&10).await.unwrap();
            match order {
                DecisionArm::Left => {
                    origin.send::<Msg<11, u32>>(&11).await.unwrap();
                    origin.send::<Msg<20, u32>>(&0xfeedbeef).await.unwrap();
                }
                DecisionArm::Right => {
                    origin.send::<Msg<20, u32>>(&0xfeedbeef).await.unwrap();
                    origin.send::<Msg<11, u32>>(&11).await.unwrap();
                }
            }
        })
        .await;
        frames.take()
    };
    assert_eq!(frames.len(), 3);
    assert_ne!(frames[0].lane, frames[2].lane);
    let carrier = TestTransport::new();
    let mut slab = [0; 128 * 1024];
    let mut storage = SessionKitStorage::<TestTransport>::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, carrier.clone())
        .unwrap();
    let program: RoleProgram<1> = project(&ordered_lanes());
    let mut receiver = rv.enter(SessionId::new(1), &program).unwrap();
    inject(&carrier, 0, &frames[2]);
    let mut cx = Context::from_waker(Waker::noop());
    assert!(matches!(
        pin!(receiver.recv::<Msg<20, u32>>()).poll(&mut cx),
        Poll::Ready(Err(_))
    ));
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn parallel_arms_remain_independent_after_the_sequential_input() {
    let frames = {
        let order = DecisionArm::Right;
        let frames = core::cell::Cell::new(Vec::new());
        let carrier = RecordingCarrier {
            inner: TestTransport::new(),
            frames: &frames,
        };
        let mut slab = [0; 128 * 1024];
        let mut storage = SessionKitStorage::<RecordingCarrier>::uninit();
        let rv = storage.init().rendezvous(&mut slab, carrier).unwrap();
        let program: RoleProgram<0> = project(&ordered_lanes());
        let mut origin = rv.enter(SessionId::new(1), &program).unwrap();
        (async {
            origin.send::<Msg<10, u32>>(&10).await.unwrap();
            match order {
                DecisionArm::Left => {
                    origin.send::<Msg<11, u32>>(&11).await.unwrap();
                    origin.send::<Msg<20, u32>>(&0xfeedbeef).await.unwrap();
                }
                DecisionArm::Right => {
                    origin.send::<Msg<20, u32>>(&0xfeedbeef).await.unwrap();
                    origin.send::<Msg<11, u32>>(&11).await.unwrap();
                }
            }
        })
        .await;
        frames.take()
    };
    assert_eq!(frames.len(), 3);
    assert_eq!(frames[1].bytes, 0xfeedbeefu32.to_be_bytes());
    assert_eq!(frames[2].bytes, 11u32.to_be_bytes());
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn a_conforming_controller_cannot_send_opposite_arms_to_two_peers() {
    let carrier = TestTransport::new();
    let mut slab = [0; 128 * 1024];
    let mut storage = SessionKitStorage::<TestTransport>::uninit();
    let rv = storage.init().rendezvous(&mut slab, carrier).unwrap();
    let program: RoleProgram<0> = project(&choice());
    rv.set_resolver(&program, ResolverRef::<900>::decision_state(&LEFT, choose))
        .unwrap();
    let mut origin = rv.enter(SessionId::new(1), &program).unwrap();
    (async {
        origin.send::<Msg<31, u32>>(&11).await.unwrap();
        assert!(origin.send::<Msg<34, u32>>(&22).await.is_err());
    })
    .await;
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn fabricated_carrier_provenance_is_not_a_byzantine_agreement_guarantee() {
    let left = {
        let arm = &LEFT;
        let frames = core::cell::Cell::new(Vec::new());
        let carrier = RecordingCarrier {
            inner: TestTransport::new(),
            frames: &frames,
        };
        let mut slab = [0; 128 * 1024];
        let mut storage = SessionKitStorage::<RecordingCarrier>::uninit();
        let rv = storage.init().rendezvous(&mut slab, carrier).unwrap();
        let program: RoleProgram<0> = project(&choice());
        rv.set_resolver(&program, ResolverRef::<900>::decision_state(arm, choose))
            .unwrap();
        let mut controller = rv.enter(SessionId::new(1), &program).unwrap();
        (async {
            match arm {
                DecisionArm::Left => {
                    controller.send::<Msg<31, u32>>(&11).await.unwrap();
                    controller.send::<Msg<32, u32>>(&12).await.unwrap();
                }
                DecisionArm::Right => {
                    controller.send::<Msg<33, u32>>(&21).await.unwrap();
                    controller.send::<Msg<34, u32>>(&22).await.unwrap();
                }
            }
        })
        .await;
        frames.take()
    };
    let right = {
        let arm = &RIGHT;
        let frames = core::cell::Cell::new(Vec::new());
        let carrier = RecordingCarrier {
            inner: TestTransport::new(),
            frames: &frames,
        };
        let mut slab = [0; 128 * 1024];
        let mut storage = SessionKitStorage::<RecordingCarrier>::uninit();
        let rv = storage.init().rendezvous(&mut slab, carrier).unwrap();
        let program: RoleProgram<0> = project(&choice());
        rv.set_resolver(&program, ResolverRef::<900>::decision_state(arm, choose))
            .unwrap();
        let mut controller = rv.enter(SessionId::new(1), &program).unwrap();
        (async {
            match arm {
                DecisionArm::Left => {
                    controller.send::<Msg<31, u32>>(&11).await.unwrap();
                    controller.send::<Msg<32, u32>>(&12).await.unwrap();
                }
                DecisionArm::Right => {
                    controller.send::<Msg<33, u32>>(&21).await.unwrap();
                    controller.send::<Msg<34, u32>>(&22).await.unwrap();
                }
            }
        })
        .await;
        frames.take()
    };
    assert_eq!((left.len(), right.len()), (2, 2));
    let carrier = TestTransport::new();
    let mut slab = [0; 128 * 1024];
    let mut storage = SessionKitStorage::<TestTransport>::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, carrier.clone())
        .unwrap();
    let p1: RoleProgram<1> = project(&choice());
    let p2: RoleProgram<2> = project(&choice());
    let mut role1 = rv.enter(SessionId::new(1), &p1).unwrap();
    let mut role2 = rv.enter(SessionId::new(1), &p2).unwrap();
    // These two observations did not come from one authorized controller
    // execution. They fabricate the absent peer's provenance in one session.
    inject(&carrier, 0, &left[0]);
    inject(&carrier, 0, &right[1]);
    (async {
        let a = role1.offer().await.unwrap();
        assert_eq!(a.label(), 31);
        assert_eq!(a.recv::<Msg<31, u32>>().await.unwrap(), 11);
        let b = role2.offer().await.unwrap();
        assert_eq!(b.label(), 34);
        assert_eq!(b.recv::<Msg<34, u32>>().await.unwrap(), 22);
    })
    .await;
}
