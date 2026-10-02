//! Regression checks for the 9758c42 / 436d0ee security reports.
//! The final test deliberately violates carrier provenance. It records the
//! documented limit; it is not a claim of Byzantine agreement enforcement.
mod common;
#[path = "security_report_regressions/rolled_routes.rs"]
mod rolled_routes;

use common::{TestTransport, TestTx};
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
use std::{cell::RefCell, rc::Rc, task::Waker};

#[derive(Clone, Debug)]
struct Captured {
    target: u8,
    lane: u8,
    label: u8,
    bytes: Vec<u8>,
}

struct RecordingCarrier {
    inner: TestTransport,
    frames: Rc<RefCell<Vec<Captured>>>,
}

impl Transport for RecordingCarrier {
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
    fn poll_send<'a, 'f>(
        &self,
        tx: &'a mut Self::Tx<'a>,
        frame: Outgoing<'f>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), TransportError>>
    where
        'a: 'f,
    {
        let result = self.inner.poll_send(tx, frame, cx);
        if matches!(result, Poll::Ready(Ok(()))) {
            self.frames.borrow_mut().push(Captured {
                target: frame.target_role(),
                lane: frame.lane(),
                label: frame.frame_label().raw(),
                bytes: frame.payload().as_bytes().to_vec(),
            });
        }
        result
    }
    fn cancel_send<'a>(&self, tx: &'a mut Self::Tx<'a>) {
        self.inner.cancel_send(tx);
    }
    fn poll_recv<'a>(
        &'a self,
        rx: &'a mut Self::Rx<'a>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<ReceivedFrame<'a>, TransportError>> {
        self.inner.poll_recv(rx, cx)
    }
    fn requeue<'a>(&self, rx: &mut Self::Rx<'a>) -> Result<(), TransportError> {
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

fn capture_ordered_lanes(order: DecisionArm) -> Vec<Captured> {
    let frames = Rc::new(RefCell::new(Vec::new()));
    let carrier = RecordingCarrier {
        inner: TestTransport::new(),
        frames: frames.clone(),
    };
    let mut slab = [0; 128 * 1024];
    let mut storage = SessionKitStorage::<RecordingCarrier>::uninit();
    let rv = storage.init().rendezvous(&mut slab, carrier).unwrap();
    let program: RoleProgram<0> = project(&ordered_lanes());
    let mut origin = rv.enter(SessionId::new(1), &program).unwrap();
    futures::executor::block_on(async {
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
    });
    frames.borrow().clone()
}

fn choose(arm: &DecisionArm) -> Result<DecisionArm, ResolverError> {
    Ok(*arm)
}
static LEFT: DecisionArm = DecisionArm::Left;
static RIGHT: DecisionArm = DecisionArm::Right;

fn capture_choice(arm: &'static DecisionArm) -> Vec<Captured> {
    let frames = Rc::new(RefCell::new(Vec::new()));
    let carrier = RecordingCarrier {
        inner: TestTransport::new(),
        frames: frames.clone(),
    };
    let mut slab = [0; 128 * 1024];
    let mut storage = SessionKitStorage::<RecordingCarrier>::uninit();
    let rv = storage.init().rendezvous(&mut slab, carrier).unwrap();
    let program: RoleProgram<0> = project(&choice());
    rv.set_resolver(&program, ResolverRef::<900>::decision_state(arm, choose))
        .unwrap();
    let mut controller = rv.enter(SessionId::new(1), &program).unwrap();
    futures::executor::block_on(async {
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
    });
    frames.borrow().clone()
}

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

#[test]
fn sequential_send_cannot_select_the_later_lane_first() {
    let frames = capture_ordered_lanes(DecisionArm::Left);
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
    assert!(futures::executor::block_on(origin.send::<Msg<20, u32>>(&0xfeedbeef)).is_err());
    assert!(
        carrier.queue_is_empty(),
        "no out-of-order frame may be published"
    );
}

#[test]
fn sequential_receive_cannot_select_the_later_lane_first() {
    let frames = capture_ordered_lanes(DecisionArm::Left);
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

#[test]
fn parallel_arms_remain_independent_after_the_sequential_input() {
    let frames = capture_ordered_lanes(DecisionArm::Right);
    assert_eq!(frames.len(), 3);
    assert_eq!(frames[1].bytes, 0xfeedbeefu32.to_be_bytes());
    assert_eq!(frames[2].bytes, 11u32.to_be_bytes());
}

#[test]
fn a_conforming_controller_cannot_send_opposite_arms_to_two_peers() {
    let carrier = TestTransport::new();
    let mut slab = [0; 128 * 1024];
    let mut storage = SessionKitStorage::<TestTransport>::uninit();
    let rv = storage.init().rendezvous(&mut slab, carrier).unwrap();
    let program: RoleProgram<0> = project(&choice());
    rv.set_resolver(&program, ResolverRef::<900>::decision_state(&LEFT, choose))
        .unwrap();
    let mut origin = rv.enter(SessionId::new(1), &program).unwrap();
    futures::executor::block_on(async {
        origin.send::<Msg<31, u32>>(&11).await.unwrap();
        assert!(origin.send::<Msg<34, u32>>(&22).await.is_err());
    });
}

#[test]
fn fabricated_carrier_provenance_is_not_a_byzantine_agreement_guarantee() {
    let left = capture_choice(&LEFT);
    let right = capture_choice(&RIGHT);
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
    futures::executor::block_on(async {
        let a = role1.offer().await.unwrap();
        assert_eq!(a.label(), 31);
        assert_eq!(a.recv::<Msg<31, u32>>().await.unwrap(), 11);
        let b = role2.offer().await.unwrap();
        assert_eq!(b.label(), 34);
        assert_eq!(b.recv::<Msg<34, u32>>().await.unwrap(), 22);
    });
}
