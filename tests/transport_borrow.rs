//! Borrowed handles with inline mutable buffers, driven through real Endpoints.
use core::{
    cell::{Cell, RefCell},
    future::Future,
    pin::pin,
    task::{Context, Poll, Waker},
};
use hibana::{
    g,
    runtime::{
        SessionKitStorage,
        ids::SessionId,
        program::project,
        transport::{Outgoing, PortOpen, ReceivedFrame, Transport, TransportError},
        wire::Payload,
    },
};
use std::collections::VecDeque;

struct Carrier {
    queue: RefCell<VecDeque<u8>>,
    polls: Cell<usize>,
}

struct Tx<'h> {
    carrier: &'h Carrier,
    buf: [u8; 1],
}

struct Rx<'h> {
    carrier: &'h Carrier,
    buf: [u8; 1],
}

impl Transport for Carrier {
    type Tx<'h> = Tx<'h>;
    type Rx<'h> = Rx<'h>;

    fn open<'h>(&'h self, _: PortOpen) -> (Self::Tx<'h>, Self::Rx<'h>) {
        (
            Tx {
                carrier: self,
                buf: [0],
            },
            Rx {
                carrier: self,
                buf: [0],
            },
        )
    }

    fn poll_send<'f>(
        &self,
        tx: &mut Self::Tx<'_>,
        frame: Outgoing<'f>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), TransportError>> {
        tx.buf.copy_from_slice(frame.payload().as_bytes());
        let count = tx.carrier.polls.get() + 1;
        tx.carrier.polls.set(count);
        if count % 2 == 1 {
            cx.waker().wake_by_ref();
            Poll::Pending
        } else {
            tx.carrier.queue.borrow_mut().push_back(tx.buf[0]);
            Poll::Ready(Ok(()))
        }
    }

    fn cancel_send(&self, tx: &mut Self::Tx<'_>) {
        tx.buf[0] = 0;
    }

    fn poll_recv<'p>(
        &'p self,
        rx: &'p mut Self::Rx<'_>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<ReceivedFrame<'p>, TransportError>> {
        let Some(value) = rx.carrier.queue.borrow_mut().pop_front() else {
            cx.waker().wake_by_ref();
            return Poll::Pending;
        };
        rx.buf[0] = value;
        Poll::Ready(Ok(ReceivedFrame::deterministic(Payload::new(&rx.buf))))
    }

    fn requeue(&self, rx: &mut Self::Rx<'_>) -> Result<(), TransportError> {
        rx.carrier.queue.borrow_mut().push_front(rx.buf[0]);
        Ok(())
    }
}

#[test]
fn borrowed_handles_support_repeated_send_receive_and_cancellation() {
    let global = g::seq(
        g::send::<0, 1, g::Msg<1, u8>>(),
        g::send::<0, 1, g::Msg<2, u8>>(),
    );
    let sender_program = project::<0>(&global);
    let receiver_program = project::<1>(&global);
    let mut slab = [0; 8192];
    let mut storage = SessionKitStorage::<Carrier>::uninit();
    let rv = storage
        .init()
        .rendezvous(
            &mut slab,
            Carrier {
                queue: RefCell::new(VecDeque::new()),
                polls: Cell::new(0),
            },
        )
        .unwrap();
    let mut sender = rv.enter(SessionId::new(9981), &sender_program).unwrap();
    let mut receiver = rv.enter(SessionId::new(9981), &receiver_program).unwrap();
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            {
                let mut cancelled = pin!(sender.send::<g::Msg<1, u8>>(&99));
                assert!(
                    cancelled
                        .as_mut()
                        .poll(&mut Context::from_waker(Waker::noop()))
                        .is_pending()
                );
            }
            let ((), ()) = tokio::join!(
                async {
                    sender.send::<g::Msg<1, u8>>(&17).await.unwrap();
                    sender.send::<g::Msg<2, u8>>(&19).await.unwrap();
                },
                async {
                    assert_eq!(receiver.recv::<g::Msg<1, u8>>().await.unwrap(), 17);
                    assert_eq!(receiver.recv::<g::Msg<2, u8>>().await.unwrap(), 19);
                },
            );
        });
}
