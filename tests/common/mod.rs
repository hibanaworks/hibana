use hibana::substrate::{
    Transport,
    transport::{TransportError, TransportEvent, TransportMetrics, TransportSnapshot},
    wire::Payload,
};
use std::{
    collections::{HashMap, VecDeque},
    future::Future,
    marker::PhantomData,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll, Waker},
};

#[derive(Clone)]
pub(crate) struct FrameOwned {
    payload: Vec<u8>,
}

#[derive(Default)]
pub(crate) struct TestState {
    pub(crate) queues: HashMap<u8, VecDeque<FrameOwned>>,
    waiters: HashMap<u8, Vec<Waker>>,
}

impl TestState {
    fn ensure_role(&mut self, role: u8) {
        self.queues.entry(role).or_default();
    }

    fn enqueue(&mut self, role: u8, frame: FrameOwned) -> Vec<Waker> {
        let queue = self.queues.entry(role).or_default();
        queue.push_back(frame);
        self.waiters.remove(&role).unwrap_or_default()
    }

    fn dequeue(&mut self, role: u8) -> Option<FrameOwned> {
        self.queues
            .get_mut(&role)
            .and_then(|queue| queue.pop_front())
    }
}

pub(crate) struct TestTx;

pub(crate) struct TestRx {
    state: Arc<Mutex<TestState>>,
    role: u8,
}

impl TestRx {
    fn key(&self) -> u8 {
        self.role
    }
}

#[derive(Clone, Default)]
pub(crate) struct TestTransport {
    pub(crate) state: Arc<Mutex<TestState>>,
    pub(crate) metrics: Arc<Mutex<TransportSnapshot>>,
}

#[derive(Debug)]
pub enum TestTransportError {
    Empty,
}

impl From<TestTransportError> for TransportError {
    fn from(err: TestTransportError) -> Self {
        match err {
            TestTransportError::Empty => TransportError::Failed,
        }
    }
}

pub(crate) struct RecvFuture<'a> {
    state: Arc<Mutex<TestState>>,
    role: u8,
    _marker: PhantomData<&'a ()>,
}

impl<'a> Future for RecvFuture<'a> {
    type Output = Result<Payload<'a>, TestTransportError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut state = self.state.lock().expect("state lock");
        if let Some(frame) = state.dequeue(self.role) {
            drop(state);
            let payload = frame.payload.into_boxed_slice();
            let leaked = Box::leak(payload);
            let payload = Payload::new(leaked);
            return Poll::Ready(Ok(payload));
        }
        state
            .waiters
            .entry(self.role)
            .or_default()
            .push(cx.waker().clone());
        Poll::Pending
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct TestTransportMetrics {
    snapshot: TransportSnapshot,
}

impl TransportMetrics for TestTransportMetrics {
    fn snapshot(&self) -> TransportSnapshot {
        self.snapshot
    }
}

impl Transport for TestTransport {
    type Error = TestTransportError;
    type Tx<'a>
        = TestTx
    where
        Self: 'a;
    type Rx<'a>
        = TestRx
    where
        Self: 'a;
    type Send<'a>
        = Pin<Box<dyn Future<Output = Result<(), Self::Error>> + 'a>>
    where
        Self: 'a;
    type Recv<'a>
        = RecvFuture<'a>
    where
        Self: 'a;
    type Metrics = TestTransportMetrics;

    fn open<'a>(&'a self, local_role: u8, _session_id: u32) -> (Self::Tx<'a>, Self::Rx<'a>) {
        let mut state = self.state.lock().expect("state lock");
        state.ensure_role(local_role);
        drop(state);
        (
            TestTx,
            TestRx {
                state: self.state.clone(),
                role: local_role,
            },
        )
    }

    fn send<'a, 'f>(
        &'a self,
        _tx: &'a mut Self::Tx<'a>,
        payload: Payload<'f>,
        dest_role: u8,
    ) -> Self::Send<'a>
    where
        'a: 'f,
    {
        let payload_vec = payload.as_bytes().to_vec();
        let state = self.state.clone();
        Box::pin(async move {
            let mut guard = state.lock().expect("state lock");
            let waiters = guard.enqueue(
                dest_role,
                FrameOwned {
                    payload: payload_vec,
                },
            );
            drop(guard);
            for waker in waiters {
                waker.wake();
            }
            Ok(())
        })
    }

    fn recv<'a>(&'a self, rx: &'a mut Self::Rx<'a>) -> Self::Recv<'a> {
        RecvFuture {
            state: rx.state.clone(),
            role: rx.key(),
            _marker: PhantomData,
        }
    }

    fn requeue<'a>(&'a self, _rx: &'a mut Self::Rx<'a>) {}

    fn drain_events(&self, _emit: &mut dyn FnMut(TransportEvent)) {}

    fn recv_label_hint<'a>(&'a self, _rx: &'a Self::Rx<'a>) -> Option<u8> {
        None
    }

    fn metrics(&self) -> Self::Metrics {
        let snapshot = *self.metrics.lock().expect("metrics lock");
        TestTransportMetrics { snapshot }
    }

    fn apply_pacing_update(&self, _interval_us: u32, _burst_bytes: u16) {}
}
