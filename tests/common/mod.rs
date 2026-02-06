use hibana::transport::{
    Transport, TransportError, TransportMetrics, TransportSnapshot, wire::Payload,
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
struct FrameOwned {
    payload: Vec<u8>,
}

#[derive(Default)]
struct TestState {
    queues: HashMap<u8, VecDeque<FrameOwned>>,
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

    fn queue_is_empty(&self) -> bool {
        self.queues.values().all(|queue| queue.is_empty())
    }
}

pub struct TestTx;

pub struct TestRx {
    state: Arc<Mutex<TestState>>,
    role: u8,
}

impl TestRx {
    fn key(&self) -> u8 {
        self.role
    }
}

#[derive(Clone, Default)]
pub struct TestTransport {
    state: Arc<Mutex<TestState>>,
    metrics: Arc<Mutex<TransportSnapshot>>,
}

impl TestTransport {
    #[allow(dead_code)]
    pub fn queue_is_empty(&self) -> bool {
        self.state.lock().expect("state lock").queue_is_empty()
    }

    #[allow(dead_code)]
    pub fn set_metrics(&self, snapshot: TransportSnapshot) {
        *self.metrics.lock().expect("metrics lock") = snapshot;
    }
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

pub struct RecvFuture<'a> {
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
pub struct TestTransportMetrics {
    snapshot: TransportSnapshot,
}

impl TransportMetrics for TestTransportMetrics {
    fn latency_us(&self) -> Option<u64> {
        self.snapshot.latency_us
    }

    fn queue_depth(&self) -> Option<u32> {
        self.snapshot.queue_depth
    }

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

    fn metrics(&self) -> Self::Metrics {
        let snapshot = *self.metrics.lock().expect("metrics lock");
        TestTransportMetrics { snapshot }
    }
}

// =============================================================================
// LossyTransport - Simulates packet loss and retransmissions
// =============================================================================

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

#[derive(Default)]
struct LossyState {
    queues: HashMap<u8, VecDeque<FrameOwned>>,
    waiters: HashMap<u8, Vec<Waker>>,
}

impl LossyState {
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

/// Transport that simulates packet loss, requiring retransmissions.
/// Tracks actual retry counts for EPF to observe.
#[derive(Clone)]
pub struct LossyTransport {
    state: Arc<Mutex<LossyState>>,
    loss_rate: u32, // Percentage 0-100
    send_count: Arc<AtomicU64>,
    drop_count: Arc<AtomicU64>,
    retry_count: Arc<AtomicU32>,
    rng_seed: Arc<AtomicU64>,
}

impl LossyTransport {
    /// Create a lossy transport with the given loss rate (0-100%).
    #[allow(dead_code)]
    pub fn new(loss_rate: u32) -> Self {
        Self {
            state: Arc::new(Mutex::new(LossyState::default())),
            loss_rate: loss_rate.min(100),
            send_count: Arc::new(AtomicU64::new(0)),
            drop_count: Arc::new(AtomicU64::new(0)),
            retry_count: Arc::new(AtomicU32::new(0)),
            rng_seed: Arc::new(AtomicU64::new(12345)),
        }
    }

    /// Get the current retry count (for verification).
    #[allow(dead_code)]
    pub fn retry_count(&self) -> u32 {
        self.retry_count.load(Ordering::Relaxed)
    }

    /// Get total sends attempted.
    #[allow(dead_code)]
    pub fn send_count(&self) -> u64 {
        self.send_count.load(Ordering::Relaxed)
    }

    /// Get total packets dropped.
    #[allow(dead_code)]
    pub fn drop_count(&self) -> u64 {
        self.drop_count.load(Ordering::Relaxed)
    }

    /// Increment retry count (simulates retransmission).
    #[allow(dead_code)]
    pub fn simulate_retry(&self) {
        self.retry_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Reset counters.
    #[allow(dead_code)]
    pub fn reset_counters(&self) {
        self.send_count.store(0, Ordering::Relaxed);
        self.drop_count.store(0, Ordering::Relaxed);
        self.retry_count.store(0, Ordering::Relaxed);
    }

    fn should_drop(&self) -> bool {
        if self.loss_rate == 0 {
            return false;
        }
        // Simple LCG random
        let seed = self.rng_seed.fetch_add(1, Ordering::Relaxed);
        let rand = (seed.wrapping_mul(6364136223846793005).wrapping_add(1)) % 100;
        rand < self.loss_rate as u64
    }
}

pub struct LossyTx;

pub struct LossyRx {
    state: Arc<Mutex<LossyState>>,
    role: u8,
}

pub struct LossyRecvFuture<'a> {
    state: Arc<Mutex<LossyState>>,
    role: u8,
    _marker: PhantomData<&'a ()>,
}

impl<'a> Future for LossyRecvFuture<'a> {
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
pub struct LossyTransportMetrics {
    retransmissions: u32,
}

impl TransportMetrics for LossyTransportMetrics {
    fn latency_us(&self) -> Option<u64> {
        None
    }

    fn queue_depth(&self) -> Option<u32> {
        None
    }

    fn snapshot(&self) -> TransportSnapshot {
        TransportSnapshot {
            retransmissions: Some(self.retransmissions),
            ..Default::default()
        }
    }
}

impl Transport for LossyTransport {
    type Error = TestTransportError;
    type Tx<'a>
        = LossyTx
    where
        Self: 'a;
    type Rx<'a>
        = LossyRx
    where
        Self: 'a;
    type Send<'a>
        = Pin<Box<dyn Future<Output = Result<(), Self::Error>> + 'a>>
    where
        Self: 'a;
    type Recv<'a>
        = LossyRecvFuture<'a>
    where
        Self: 'a;
    type Metrics = LossyTransportMetrics;

    fn open<'a>(&'a self, local_role: u8, _session_id: u32) -> (Self::Tx<'a>, Self::Rx<'a>) {
        let mut state = self.state.lock().expect("state lock");
        state.ensure_role(local_role);
        drop(state);
        (
            LossyTx,
            LossyRx {
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
        self.send_count.fetch_add(1, Ordering::Relaxed);
        let payload_vec = payload.as_bytes().to_vec();
        let state = self.state.clone();
        let should_drop = self.should_drop();
        let drop_count = self.drop_count.clone();
        let retry_count = self.retry_count.clone();

        Box::pin(async move {
            if should_drop {
                // Packet "lost" - increment counters but still deliver
                // (simulating eventual delivery after retries)
                drop_count.fetch_add(1, Ordering::Relaxed);
                retry_count.fetch_add(1, Ordering::Relaxed);
            }

            // Always deliver (simulates successful retry)
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
        LossyRecvFuture {
            state: rx.state.clone(),
            role: rx.role,
            _marker: PhantomData,
        }
    }

    fn metrics(&self) -> Self::Metrics {
        LossyTransportMetrics {
            retransmissions: self.retry_count.load(Ordering::Relaxed),
        }
    }
}
