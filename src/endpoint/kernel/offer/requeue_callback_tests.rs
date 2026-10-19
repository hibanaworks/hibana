use core::{
    cell::{Cell, RefCell},
    task::{Context, Poll},
};
use std::rc::Rc;

use crate::{
    endpoint::kernel::{ResidentActor, lane_port},
    g::{self, Msg},
    runtime::{
        SessionKitStorage,
        ids::SessionId,
        program::{RoleProgram, project},
    },
    transport::{
        FrameHeader, Outgoing, PortOpen, ReceivedFrame, Transport, TransportError, wire::Payload,
    },
};

const REQUEUE_LABEL: u8 = 91;

#[repr(align(16))]
struct AlignedSlab([u8; 65_536]);

fn program<const ROLE: u8>() -> RoleProgram<ROLE> {
    project(&g::send::<0, 1, Msg<REQUEUE_LABEL, u32>>())
}

struct DropEndpointState {
    target: Cell<*mut ()>,
    drop_target: Cell<Option<unsafe fn(*mut ())>>,
    fired: Cell<bool>,
}

impl DropEndpointState {
    fn empty() -> Self {
        Self {
            target: Cell::new(core::ptr::null_mut()),
            drop_target: Cell::new(None),
            fired: Cell::new(false),
        }
    }

    fn arm<T>(&self, target: &RefCell<Option<T>>) {
        self.target
            .set(core::ptr::from_ref(target).cast_mut().cast::<()>());
        self.drop_target.set(Some(drop_option::<T>));
    }

    fn fire(&self) {
        if self.fired.replace(true) {
            return;
        }
        let drop_target = self.drop_target.get().expect("armed drop callback");
        let target = self.target.get();
        assert!(!target.is_null(), "armed drop target");
        unsafe {
            // SAFETY: `arm` pairs this live target pointer with its
            // monomorphized drop callback, and `fired` permits one call.
            drop_target(target);
        }
    }
}

unsafe fn drop_option<T>(target: *mut ()) {
    let target = unsafe {
        // SAFETY: `arm` stored a pointer to this live, stationary RefCell
        // together with the matching monomorphized callback. The closure can
        // inspect it after reentry without aliasing the mutation in `take`.
        &*target.cast::<RefCell<Option<T>>>()
    };
    drop(target.borrow_mut().take());
}

struct ReentrantRequeueTransport {
    state: Rc<DropEndpointState>,
    bytes: [u8; 4],
}

struct ReentrantRequeueRx {
    session_id: SessionId,
    lane: u8,
    local_role: u8,
    delivered: bool,
}

impl Transport for ReentrantRequeueTransport {
    type Tx<'a> = ();
    type Rx<'a> = ReentrantRequeueRx;

    fn open<'a>(&'a self, port: PortOpen) -> (Self::Tx<'a>, Self::Rx<'a>) {
        (
            (),
            ReentrantRequeueRx {
                session_id: port.session_id(),
                lane: port.lane(),
                local_role: port.local_role(),
                delivered: false,
            },
        )
    }

    fn poll_send<'f>(
        &self,
        _: &mut Self::Tx<'_>,
        _: Outgoing<'f>,
        _: &mut Context<'_>,
    ) -> Poll<Result<(), TransportError>> {
        Poll::Ready(Ok(()))
    }

    fn cancel_send(&self, _: &mut Self::Tx<'_>) {}

    fn poll_recv<'a>(
        &'a self,
        rx: &'a mut Self::Rx<'_>,
        _: &mut Context<'_>,
    ) -> Poll<Result<ReceivedFrame<'a>, TransportError>> {
        if rx.delivered {
            return Poll::Pending;
        }
        rx.delivered = true;
        let session = rx.session_id.raw().to_be_bytes();
        let header = FrameHeader::from_bytes([
            session[0],
            session[1],
            session[2],
            session[3],
            rx.lane,
            0,
            rx.local_role,
            REQUEUE_LABEL,
        ]);
        Poll::Ready(Ok(ReceivedFrame::framed(header, Payload::new(&self.bytes))))
    }

    fn requeue<'a>(&self, rx: &mut Self::Rx<'a>) -> Result<(), TransportError> {
        assert!(rx.delivered, "requeue requires an outstanding frame");
        rx.delivered = false;
        self.state.fire();
        Ok(())
    }
}

#[test]
fn transport_requeue_callback_reentry_revalidates_generation() {
    let role0 = program::<0>();
    let role1 = program::<1>();
    let mut slab = AlignedSlab([0; 65_536]);
    let state = Rc::new(DropEndpointState::empty());
    let mut storage = SessionKitStorage::<ReentrantRequeueTransport>::uninit();
    let kit = storage.init();
    let rendezvous = kit
        .rendezvous(
            &mut slab.0,
            ReentrantRequeueTransport {
                state: Rc::clone(&state),
                bytes: 91u32.to_be_bytes(),
            },
        )
        .expect("register rendezvous");
    let sid = SessionId::new(8);
    let peer = RefCell::new(Some(rendezvous.enter(sid, &role0).expect("peer")));
    let target = rendezvous.enter(sid, &role1).expect("target");
    state.arm(&peer);

    let actor = unsafe {
        // SAFETY: the issued view retains this exact pinned actor root.
        target
            .owner
            .ptr
            .as_ref()
            .actor()
            .cast::<ResidentActor<'_, ReentrantRequeueTransport>>()
            .as_ref()
    };
    let mut operation_lease = actor.context.try_operation_lease().expect("kernel lease");
    actor.with_kernel(
        &mut operation_lease,
        actor.views.get(1).unwrap(),
        crate::endpoint::carrier::OperationRegion::WHOLE,
        |kernel| {
            let lane_idx = kernel
                .views
                .get(kernel.public_header.role())
                .unwrap()
                .primary_lane;
            let lane_wire = kernel.port_for_lane(lane_idx).lane().as_wire();
            let mut pending_recv = lane_port::PendingRecv::new();
            let waker = futures::task::noop_waker_ref();
            let mut context = Context::from_waker(waker);
            let frame = match kernel.poll_received_framed_transport_frame_for_lane(
                &mut pending_recv,
                lane_idx,
                lane_wire,
                &mut context,
            ) {
                Poll::Ready(Ok(frame)) => frame,
                Poll::Ready(Err(error)) => panic!("receive frame failed: {error:?}"),
                Poll::Pending => panic!("test transport must return one frame"),
            };

            let error = kernel
                .requeue_offer_transport_payload(frame)
                .expect_err("requeue callback must expose the peer-drop session fault");

            assert!(state.fired.get());
            assert!(peer.borrow().is_none());
            assert!(matches!(
                error,
                crate::endpoint::RecvError::SessionFault(
                    crate::rendezvous::SessionFaultKind::EndpointDropped
                )
            ));
        },
    );
    drop(operation_lease);
    drop(target);
}
