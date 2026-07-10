use core::{
    cell::Cell,
    task::{Context, Poll},
};
use std::rc::Rc;

use crate::{
    endpoint::kernel::{CursorEndpoint, lane_port},
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

    fn arm<T>(&self, target: &mut Option<T>) {
        self.target.set(core::ptr::from_mut(target).cast::<()>());
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
        // SAFETY: `DropEndpointState::arm` stored the unique pointer to this
        // live `Option<T>` together with this monomorphized callback.
        &mut *target.cast::<Option<T>>()
    };
    drop(target.take());
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

    fn poll_send<'a, 'f>(
        &self,
        _: &'a mut Self::Tx<'a>,
        _: Outgoing<'f>,
        _: &mut Context<'_>,
    ) -> Poll<Result<(), TransportError>>
    where
        'a: 'f,
    {
        Poll::Ready(Ok(()))
    }

    fn cancel_send<'a>(&self, _: &'a mut Self::Tx<'a>) {}

    fn poll_recv<'a>(
        &'a self,
        rx: &'a mut Self::Rx<'a>,
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
    let mut peer = Some(rendezvous.enter(sid, &role0).expect("peer"));
    let target = rendezvous.enter(sid, &role1).expect("target");
    state.arm(&mut peer);

    let kernel_ptr = target
        .ptr
        .cast::<CursorEndpoint<'_, 1, ReentrantRequeueTransport>>();
    let kernel = unsafe {
        // SAFETY: the public endpoint header is the first `repr(C)` field of
        // this exact role/transport kernel, and `target` owns it exclusively.
        &mut *kernel_ptr.as_ptr()
    };
    let lane_idx = kernel.primary_lane;
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
    assert!(peer.is_none());
    assert!(matches!(
        error,
        crate::endpoint::RecvError::SessionFault(
            crate::rendezvous::SessionFaultKind::EndpointDropped
        )
    ));
    drop(target);
}
