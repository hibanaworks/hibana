use super::*;
use crate::{
    test_support::transport::{TestRx, TestTx},
    transport::{Outgoing, PortOpen, ReceivedFrame, Transport, TransportError},
};
use core::{
    cell::{Cell, RefCell},
    ptr::NonNull,
    task::{Context, Poll},
};
use std::{boxed::Box, rc::Rc};

type DropAction = (NonNull<()>, unsafe fn(NonNull<()>));

/// One external I/O callback. Its erased pointer is used only while the test
/// keeps the stationary target alive, and is removed before invoking Drop.
struct DropHook(Cell<Option<DropAction>>);

impl DropHook {
    fn arm<T>(&self, target: &RefCell<Option<T>>) {
        unsafe fn drop_target<T>(ptr: NonNull<()>) {
            // SAFETY: arm pairs this pointer with its exact live RefCell type.
            let target = unsafe { ptr.cast::<RefCell<Option<T>>>().as_ref() };
            let value = target.borrow_mut().take();
            drop(value);
        }
        assert!(self.0.get().is_none());
        self.0
            .set(Some((NonNull::from(target).cast(), drop_target::<T>)));
    }

    fn fire(&self) {
        if let Some((ptr, callback)) = self.0.take() {
            // SAFETY: the target stays live until this one-shot callback fires.
            unsafe { callback(ptr) };
        }
    }
}

struct DropOnSend {
    inner: TestTransport,
    hook: Rc<DropHook>,
}

impl Transport for DropOnSend {
    type Tx<'a>
        = TestTx
    where
        Self: 'a;
    type Rx<'a>
        = TestRx<'a>
    where
        Self: 'a;

    fn open<'a>(&'a self, port: PortOpen) -> (Self::Tx<'a>, Self::Rx<'a>) {
        self.inner.open(port)
    }

    fn poll_send<'f>(
        &self,
        tx: &mut Self::Tx<'_>,
        out: Outgoing<'f>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), TransportError>> {
        self.hook.fire();
        self.inner.poll_send(tx, out, cx)
    }

    fn cancel_send(&self, tx: &mut Self::Tx<'_>) {
        self.inner.cancel_send(tx);
    }

    fn poll_recv<'a>(
        &'a self,
        rx: &'a mut Self::Rx<'_>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<ReceivedFrame<'a>, TransportError>> {
        self.inner.poll_recv(rx, cx)
    }

    fn requeue<'a>(&self, rx: &mut Self::Rx<'a>) -> Result<(), TransportError> {
        self.inner.requeue(rx)
    }
}

#[test]
fn a_reentrant_arm_drop_cancels_and_wakes_its_pending_sibling() {
    type Left = g::Msg<131, u32>;
    type Right = g::Msg<132, u32>;
    let global = g::par(g::send::<0, 1, Left>(), g::send::<0, 1, Right>());
    let program = project::<0>(&global);
    let network = TestTransport::new();
    let hook = Rc::new(DropHook(Cell::new(None)));
    let mut slab = [0; 32 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(
            &mut slab,
            DropOnSend {
                inner: network.clone(),
                hook: hook.clone(),
            },
        )
        .unwrap();
    let mut endpoint = rv.enter(SessionId::new(1653), &program).unwrap();
    let [mut left, right] = endpoint.par().unwrap();
    let target = RefCell::new(Some(right));
    hook.arm(&target);
    network.pause_send(0);
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            let sent =
                tokio::time::timeout(core::time::Duration::from_secs(10), left.send::<Left>(&7))
                    .await;
            assert!(
                sent.is_ok(),
                "a drop inside poll_send must not lose cancellation or its wakeup"
            );
            assert!(
                sent.unwrap().is_err(),
                "the incomplete sibling ended the run"
            );
        });
    assert!(target.borrow().is_none());
    assert!(hook.0.get().is_none());
    assert!(network.queue_is_empty());
}

#[test]
fn a_reentrant_completed_arm_drop_preserves_its_pending_sibling() {
    type Request = g::Msg<133, u32>;
    type Notice = g::Msg<134, u32>;
    let global = g::par(g::send::<0, 1, Request>(), g::send::<0, 1, Notice>());
    let client_program = project::<0>(&global);
    let server_program = project::<1>(&global);
    let network = TestTransport::new();
    let hook = Rc::new(DropHook(Cell::new(None)));
    let mut slab = [0; 32 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(
            &mut slab,
            DropOnSend {
                inner: network.clone(),
                hook: hook.clone(),
            },
        )
        .unwrap();
    let sid = SessionId::new(1655);
    let mut client = rv.enter(sid, &client_program).unwrap();
    let mut server = rv.enter(sid, &server_program).unwrap();
    let [mut request, mut notice] = client.par().unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            notice.send::<Notice>(&9).await.unwrap();
            assert_eq!(server.recv::<Notice>().await.unwrap(), 9);
            let target = RefCell::new(Some(notice));
            hook.arm(&target);
            network.pause_send(0);
            tokio::time::timeout(core::time::Duration::from_secs(10), async {
                let (sent, ()) = tokio::join!(request.send::<Request>(&7), async {
                    tokio::time::sleep(core::time::Duration::from_secs(1)).await;
                    network.resume_send();
                });
                sent.expect("dropping a completed arm must not cancel another arm");
                assert_eq!(server.recv::<Request>().await.unwrap(), 7);
            })
            .await
            .expect("the completed arm's destruction must not lose a wakeup");
            assert!(target.borrow().is_none());
        });
    assert!(hook.0.get().is_none());
    assert!(network.queue_is_empty());
}

#[test]
fn multiple_reentrant_arm_drops_are_all_drained() {
    type Request = g::Msg<135, u32>;
    type First = g::Msg<136, u32>;
    type Second = g::Msg<137, u32>;
    let global = g::par(
        g::send::<0, 1, Request>(),
        g::par(g::send::<0, 1, First>(), g::send::<0, 1, Second>()),
    );
    let program = project::<0>(&global);
    let network = TestTransport::new();
    let hook = Rc::new(DropHook(Cell::new(None)));
    let mut slab = [0; 32 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(
            &mut slab,
            DropOnSend {
                inner: network.clone(),
                hook: hook.clone(),
            },
        )
        .unwrap();
    let mut client = rv.enter(SessionId::new(1656), &program).unwrap();
    let [mut request, first, second] = client.par().unwrap();
    let target = RefCell::new(Some((first, second)));
    hook.arm(&target);
    network.pause_send(0);
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            let sent = tokio::time::timeout(
                core::time::Duration::from_secs(10),
                request.send::<Request>(&7),
            )
            .await
            .expect("both queued arm drops must be reconciled");
            assert!(sent.is_err(), "the incomplete siblings ended the run");
        });
    assert!(target.borrow().is_none());
    assert!(hook.0.get().is_none());
    assert!(network.queue_is_empty());
}
#[test]
fn a_reentrant_future_drop_releases_its_operation_before_reuse() {
    use core::{future::Future, task::Poll};
    type Request = g::Msg<138, u32>;
    type Notice = g::Msg<139, u32>;
    let global = g::par(g::send::<0, 1, Request>(), g::send::<0, 1, Notice>());
    let client_program = project::<0>(&global);
    let server_program = project::<1>(&global);
    let network = TestTransport::new();
    let hook = Rc::new(DropHook(Cell::new(None)));
    let mut slab = [0; 32 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(
            &mut slab,
            DropOnSend {
                inner: network.clone(),
                hook: hook.clone(),
            },
        )
        .unwrap();
    let sid = SessionId::new(1657);
    let mut client = rv.enter(sid, &client_program).unwrap();
    let mut server = rv.enter(sid, &server_program).unwrap();
    let [mut request, mut notice] = client.par().unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            network.pause_send(0);
            let mut pending = Box::pin(notice.send::<Notice>(&3));
            core::future::poll_fn(|cx| {
                assert!(pending.as_mut().poll(cx).is_pending());
                Poll::Ready(())
            })
            .await;
            let target = RefCell::new(Some(pending));
            hook.arm(&target);
            tokio::time::timeout(core::time::Duration::from_secs(10), async {
                let (sent, ()) = tokio::join!(request.send::<Request>(&7), async {
                    tokio::time::sleep(core::time::Duration::from_secs(1)).await;
                    network.resume_send();
                });
                sent.unwrap();
                assert_eq!(server.recv::<Request>().await.unwrap(), 7);
            })
            .await
            .expect("the surviving operation should finish");
            assert!(target.borrow().is_none());
            drop(target);
            notice
                .send::<Notice>(&9)
                .await
                .expect("dropping its future must release the operation");
            assert_eq!(server.recv::<Notice>().await.unwrap(), 9);
        });
    assert!(hook.0.get().is_none());
    assert!(network.queue_is_empty());
}

#[test]
fn a_reentrant_receive_drop_allows_the_same_step_to_be_received() {
    use core::future::Future;
    type Request = g::Msg<140, u32>;
    type Notice = g::Msg<141, u32>;
    let global = g::par(g::send::<0, 1, Request>(), g::send::<1, 0, Notice>());
    let client_program = project::<0>(&global);
    let server_program = project::<1>(&global);
    let network = TestTransport::new();
    let hook = Rc::new(DropHook(Cell::new(None)));
    let mut slab = [0; 32 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(
            &mut slab,
            DropOnSend {
                inner: network.clone(),
                hook: hook.clone(),
            },
        )
        .unwrap();
    let sid = SessionId::new(1659);
    let mut client = rv.enter(sid, &client_program).unwrap();
    let mut server = rv.enter(sid, &server_program).unwrap();
    let [mut request, mut notice] = client.par().unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(core::time::Duration::from_secs(10), async {
                let mut pending = Box::pin(notice.recv::<Notice>());
                core::future::poll_fn(|cx| {
                    assert!(pending.as_mut().poll(cx).is_pending());
                    Poll::Ready(())
                })
                .await;
                let target = RefCell::new(Some(pending));
                hook.arm(&target);
                request.send::<Request>(&7).await.unwrap();
                assert!(target.borrow().is_none());
                drop(target);
                assert_eq!(server.recv::<Request>().await.unwrap(), 7);
                server.send::<Notice>(&9).await.unwrap();
                assert_eq!(notice.recv::<Notice>().await.unwrap(), 9);
            })
            .await
            .expect("dropping the pending receive must release its operation");
        });
    assert!(hook.0.get().is_none());
    assert!(network.queue_is_empty());
}

#[test]
fn a_reentrant_branch_drop_preserves_its_selected_frame() {
    type Request = g::Msg<142, u32>;
    type Notice = g::Msg<143, u32>;
    type Other = g::Msg<144, u32>;
    let global = g::par(
        g::send::<0, 1, Request>(),
        g::route(g::send::<1, 0, Notice>(), g::send::<1, 0, Other>()),
    );
    let client_program = project::<0>(&global);
    let server_program = project::<1>(&global);
    let network = TestTransport::new();
    let hook = Rc::new(DropHook(Cell::new(None)));
    let mut slab = [0; 32 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(
            &mut slab,
            DropOnSend {
                inner: network.clone(),
                hook: hook.clone(),
            },
        )
        .unwrap();
    let sid = SessionId::new(1660);
    let mut client = rv.enter(sid, &client_program).unwrap();
    let mut server = rv.enter(sid, &server_program).unwrap();
    let [mut request, mut notice] = client.par().unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(core::time::Duration::from_secs(10), async {
                server.send::<Notice>(&9).await.unwrap();
                let branch = notice.offer().await.unwrap();
                assert_eq!(branch.label(), 143);
                let target = RefCell::new(Some(branch));
                hook.arm(&target);
                request.send::<Request>(&7).await.unwrap();
                assert!(target.borrow().is_none());
                drop(target);
                let branch = notice
                    .offer()
                    .await
                    .expect("the selected branch must be restored");
                assert_eq!(branch.label(), 143);
                assert_eq!(branch.recv::<Notice>().await.unwrap(), 9);
                assert_eq!(server.recv::<Request>().await.unwrap(), 7);
            })
            .await
            .expect("dropping a preview must not consume its selected frame");
        });
    assert!(hook.0.get().is_none());
    assert!(network.queue_is_empty());
}
