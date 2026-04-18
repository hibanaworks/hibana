//! Transport abstraction bridging Hibana frames onto concrete mediums.
//!
//! Implementations are expected to integrate with external async runtimes by
//! returning `Future`s that complete once I/O finishes. Hibana never polls
//! transports directly; callers drive futures to completion using the runtime
//! that suits their environment (unikernel, hypervisor, user-space, ...).
//!
//! Receive buffers must be exposed as borrowed views. The rendezvous layer
//! provides a slab (see [`crate::runtime::config::Config::slab`]) that transports can pin
//! behind their `Rx` handle so [`Transport::recv`] yields payload views borrowed
//! from that storage. This keeps the runtime allocation-free while allowing
//! DMA/SHM backed zero-copy paths.
//!
//! Implementations also bridge device interrupts to the task waker stored by
//! the futures returned from [`Transport::send`] and [`Transport::recv`]. When a
//! future parks it must record the current [`core::task::Waker`] so the interrupt
//! handler can call `wake_by_ref` instead of relying on polling loops.

use core::future::Future;

use crate::{
    eff::EffIndex,
    transport::wire::{CodecError, Payload, WireEncode, WirePayload},
};

/// Congestion control algorithm observed by a transport.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportAlgorithm {
    Cubic,
    Reno,
    Other(u8),
}

/// Snapshot of transport-level observations supplied to routing policies.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TransportSnapshot {
    /// Estimated one-way latency in microseconds.
    pub latency_us: Option<u64>,
    /// Estimated queue depth for pending frames.
    pub queue_depth: Option<u32>,
    /// Suggested pacing interval between packet transmissions in microseconds.
    pub pacing_interval_us: Option<u64>,
    /// Count of congestion marks (e.g. ECN-CE) observed within the sampling window.
    pub congestion_marks: Option<u32>,
    /// Count of retransmissions (or retry attempts) in the sampling window.
    pub retransmissions: Option<u32>,
    /// Count of PTO (Probe Timeout) events observed by the recovery pipeline.
    pub pto_count: Option<u32>,
    /// Smoothed RTT estimate (per RFC 9002) in microseconds.
    pub srtt_us: Option<u64>,
    /// Most recent acknowledged packet number (1-RTT space).
    pub latest_ack_pn: Option<u64>,
    /// Congestion window estimate in bytes.
    pub congestion_window: Option<u64>,
    /// Bytes currently counted as in-flight at the transport level.
    pub in_flight_bytes: Option<u64>,
    /// Congestion control algorithm in effect (if known).
    pub algorithm: Option<TransportAlgorithm>,
}

impl TransportSnapshot {
    /// Construct a snapshot from optional latency and queue depth readings.
    ///
    /// Additional counters default to `None` and can be populated via the
    /// builder-style helpers (`with_congestion_marks`, `with_retransmissions`).
    pub const fn new(latency_us: Option<u64>, queue_depth: Option<u32>) -> Self {
        let snapshot = Self {
            latency_us,
            queue_depth,
            pacing_interval_us: None,
            congestion_marks: None,
            retransmissions: None,
            pto_count: None,
            srtt_us: None,
            latest_ack_pn: None,
            congestion_window: None,
            in_flight_bytes: None,
            algorithm: None,
        };
        snapshot
    }

    /// Attach congestion mark statistics (ECN-CE or equivalent) to the snapshot.
    pub const fn with_congestion_marks(mut self, congestion_marks: Option<u32>) -> Self {
        self.congestion_marks = congestion_marks;
        self
    }

    /// Attach a pacing interval recommendation (microseconds between packets).
    pub const fn with_pacing_interval(mut self, pacing_interval_us: Option<u64>) -> Self {
        self.pacing_interval_us = pacing_interval_us;
        self
    }

    /// Attach retransmission statistics to the snapshot.
    pub const fn with_retransmissions(mut self, retransmissions: Option<u32>) -> Self {
        self.retransmissions = retransmissions;
        self
    }

    /// Attach PTO count statistics to the snapshot.
    pub const fn with_pto_count(mut self, pto_count: Option<u32>) -> Self {
        self.pto_count = pto_count;
        self
    }

    /// Attach an RTT estimate (Smoothed RTT in microseconds).
    pub const fn with_srtt(mut self, srtt_us: Option<u64>) -> Self {
        self.srtt_us = srtt_us;
        self
    }

    /// Attach the most recent acknowledged packet number.
    pub const fn with_latest_ack(mut self, latest_ack_pn: Option<u64>) -> Self {
        self.latest_ack_pn = latest_ack_pn;
        self
    }

    /// Attach a congestion window estimate (bytes) to the snapshot.
    pub const fn with_congestion_window(mut self, congestion_window: Option<u64>) -> Self {
        self.congestion_window = congestion_window;
        self
    }

    /// Attach the number of bytes currently considered in flight.
    pub const fn with_in_flight(mut self, in_flight_bytes: Option<u64>) -> Self {
        self.in_flight_bytes = in_flight_bytes;
        self
    }

    /// Attach the congestion control algorithm affecting this snapshot.
    pub const fn with_algorithm(mut self, algorithm: Option<TransportAlgorithm>) -> Self {
        self.algorithm = algorithm;
        self
    }

    /// Encode the snapshot into transport metrics tap arguments.
    ///
    /// The primary tuple encodes algorithm/queue depth/SRTT and congestion window/in-flight
    /// counters. When additional fields are available (retransmissions, congestion marks,
    /// pacing interval), a secondary tuple is produced which callers emit using the
    /// `ids::TRANSPORT_METRICS_EXT` tap identifier.
    ///
    /// * `arg0` — `[ algo | queue_depth | srtt_scaled ]`
    ///   * bits 31-28 store the algorithm identifier (0 reserved)
    ///   * bits 27-16 store the queue depth (saturated to 12 bits)
    ///   * bits 15-0 store `srtt_us / 32` (saturated to 16 bits)
    /// * `arg1` — `[ congestion_window_kib | in_flight_kib ]`
    ///   * bits 31-16 store the congestion window in KiB (saturated to 16 bits)
    ///   * bits 15-0 store in-flight bytes in KiB (saturated to 16 bits)
    pub fn encode_tap_metrics(&self) -> Option<TransportMetricsTapPayload> {
        let algorithm = self.algorithm?;
        let algo_bits = match algorithm {
            TransportAlgorithm::Cubic => 1u32,
            TransportAlgorithm::Reno => 2u32,
            TransportAlgorithm::Other(code) => (code as u32).min(0xF).max(1),
        };
        let queue_depth = self
            .queue_depth
            .map(|value| value.min(0x0FFE) + 1)
            .unwrap_or(0);
        let srtt_units = self
            .srtt_us
            .map(|value| ((value / 32).min(0xFFFE) as u32) + 1)
            .unwrap_or(0);
        let congestion_window = self
            .congestion_window
            .map(|bytes| ((bytes / 1024).min(0xFFFE) as u32) + 1)
            .unwrap_or(0);
        let in_flight = self
            .in_flight_bytes
            .map(|bytes| ((bytes / 1024).min(0xFFFE) as u32) + 1)
            .unwrap_or(0);
        let arg0 = (algo_bits << 28) | (queue_depth << 16) | srtt_units;
        let arg1 = (congestion_window << 16) | in_flight;
        let extension_needed = self.retransmissions.is_some()
            || self.congestion_marks.is_some()
            || self.pacing_interval_us.is_some();
        let extension = if extension_needed {
            let retransmissions = self
                .retransmissions
                .map(|value| value.min(0xFFFE) + 1)
                .unwrap_or(0);
            let congestion_marks = self
                .congestion_marks
                .map(|value| value.min(0xFFFE) + 1)
                .unwrap_or(0);
            let pacing_interval = self
                .pacing_interval_us
                .map(|value| {
                    let clamped = value.min(u32::MAX as u64 - 1);
                    (clamped as u32) + 1
                })
                .unwrap_or(0);
            let ext_arg0 = (retransmissions << 16) | congestion_marks;
            Some((ext_arg0, pacing_interval))
        } else {
            None
        };
        Some(TransportMetricsTapPayload {
            primary: (arg0, arg1),
            extension,
        })
    }
}

/// Packed tap payload emitted for transport metrics sampling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransportMetricsTapPayload {
    pub primary: (u32, u32),
    pub extension: Option<(u32, u32)>,
}

/// Metrics facade returned by transports to feed routing SLO checks.
pub trait TransportMetrics {
    /// Convert the current readings into a compact snapshot.
    fn snapshot(&self) -> TransportSnapshot;
}

impl TransportMetrics for () {
    fn snapshot(&self) -> TransportSnapshot {
        TransportSnapshot::new(None, None)
    }
}

/// Direction of a send operation from the local role's perspective.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalDirection {
    /// Sending to a peer over the transport.
    Send,
    /// Metadata describes the receive-side mirror of a transport action.
    Recv,
    /// Local-only self-send that must not hit the wire.
    Local,
}

/// Transport-owned metadata for an outgoing payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SendMeta {
    /// Effect index (stable identifier for the choreography step).
    pub eff_index: EffIndex,
    /// Message label.
    pub label: u8,
    /// Target peer role.
    pub peer: u8,
    /// Logical lane for this message.
    pub lane: u8,
    /// Direction from the local role's perspective.
    pub direction: LocalDirection,
    /// Whether this is a control message.
    pub is_control: bool,
}

impl SendMeta {
    #[inline]
    pub const fn is_send(&self) -> bool {
        matches!(self.direction, LocalDirection::Send)
    }

    #[inline]
    pub const fn is_recv(&self) -> bool {
        matches!(self.direction, LocalDirection::Recv)
    }

    #[inline]
    pub const fn is_local(&self) -> bool {
        matches!(self.direction, LocalDirection::Local)
    }
}

/// Transport-owned outgoing frame.
#[derive(Debug)]
pub struct Outgoing<'f> {
    pub meta: SendMeta,
    pub payload: Payload<'f>,
}

/// Semantic classification for transport-level telemetry events.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportEventKind {
    Ack,
    Loss,
    KeepaliveTx,
    KeepaliveRx,
    CloseStart,
    CloseDraining,
    CloseRemote,
    Timeout,
}

impl WireEncode for TransportEventKind {
    fn encoded_len(&self) -> Option<usize> {
        Some(1)
    }

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.is_empty() {
            return Err(CodecError::Truncated);
        }
        out[0] = match self {
            TransportEventKind::Ack => 0,
            TransportEventKind::Loss => 1,
            TransportEventKind::KeepaliveTx => 2,
            TransportEventKind::KeepaliveRx => 3,
            TransportEventKind::CloseStart => 4,
            TransportEventKind::CloseDraining => 5,
            TransportEventKind::CloseRemote => 6,
            TransportEventKind::Timeout => 7,
        };
        Ok(1)
    }
}

impl WirePayload for TransportEventKind {
    type Decoded<'a> = Self;

    fn decode_payload<'a>(input: Payload<'a>) -> Result<Self::Decoded<'a>, CodecError> {
        let bytes = input.as_bytes();
        if bytes.is_empty() {
            return Err(CodecError::Truncated);
        }
        match bytes[0] {
            0 => Ok(TransportEventKind::Ack),
            1 => Ok(TransportEventKind::Loss),
            2 => Ok(TransportEventKind::KeepaliveTx),
            3 => Ok(TransportEventKind::KeepaliveRx),
            4 => Ok(TransportEventKind::CloseStart),
            5 => Ok(TransportEventKind::CloseDraining),
            6 => Ok(TransportEventKind::CloseRemote),
            7 => Ok(TransportEventKind::Timeout),
            _ => Err(CodecError::Invalid("transport event kind")),
        }
    }
}

/// Telemetry describing an acknowledged or lost packet emitted by a transport.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransportEvent {
    pub kind: TransportEventKind,
    pub packet_number: u64,
    pub payload_len: u32,
    pub retransmissions: u32,
    /// Packet number space identifier (transport-defined).
    pub pn_space: u8,
    /// Truncated tag identifying the relevant connection identifier (transport-defined).
    pub cid_tag: u8,
}

impl WireEncode for TransportEvent {
    fn encoded_len(&self) -> Option<usize> {
        Some(19)
    }

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        const LEN: usize = 19;
        if out.len() < LEN {
            return Err(CodecError::Truncated);
        }
        out[0] = match self.kind {
            TransportEventKind::Ack => 0,
            TransportEventKind::Loss => 1,
            TransportEventKind::KeepaliveTx => 2,
            TransportEventKind::KeepaliveRx => 3,
            TransportEventKind::CloseStart => 4,
            TransportEventKind::CloseDraining => 5,
            TransportEventKind::CloseRemote => 6,
            TransportEventKind::Timeout => 7,
        };
        out[1] = self.pn_space;
        out[2] = self.cid_tag;
        out[3..11].copy_from_slice(&self.packet_number.to_be_bytes());
        out[11..15].copy_from_slice(&self.payload_len.to_be_bytes());
        out[15..19].copy_from_slice(&self.retransmissions.to_be_bytes());
        Ok(LEN)
    }
}

impl WirePayload for TransportEvent {
    type Decoded<'a> = Self;

    fn decode_payload<'a>(input: Payload<'a>) -> Result<Self::Decoded<'a>, CodecError> {
        const LEN: usize = 19;
        let bytes = input.as_bytes();
        if bytes.len() < LEN {
            return Err(CodecError::Truncated);
        }
        let kind = match bytes[0] {
            0 => TransportEventKind::Ack,
            1 => TransportEventKind::Loss,
            2 => TransportEventKind::KeepaliveTx,
            3 => TransportEventKind::KeepaliveRx,
            4 => TransportEventKind::CloseStart,
            5 => TransportEventKind::CloseDraining,
            6 => TransportEventKind::CloseRemote,
            7 => TransportEventKind::Timeout,
            _ => return Err(CodecError::Invalid("transport event kind")),
        };
        let pn_space = bytes[1];
        let cid_tag = bytes[2];
        let mut pn_bytes = [0u8; 8];
        pn_bytes.copy_from_slice(&bytes[3..11]);
        let mut payload_bytes = [0u8; 4];
        payload_bytes.copy_from_slice(&bytes[11..15]);
        let mut retrans_bytes = [0u8; 4];
        retrans_bytes.copy_from_slice(&bytes[15..19]);
        Ok(TransportEvent {
            kind,
            packet_number: u64::from_be_bytes(pn_bytes),
            payload_len: u32::from_be_bytes(payload_bytes),
            retransmissions: u32::from_be_bytes(retrans_bytes),
            pn_space,
            cid_tag,
        })
    }
}

impl TransportEvent {
    pub const fn new(
        kind: TransportEventKind,
        packet_number: u64,
        payload_len: u32,
        retransmissions: u32,
    ) -> Self {
        Self::new_with_metadata(kind, packet_number, payload_len, retransmissions, 0, 0)
    }

    pub const fn new_with_metadata(
        kind: TransportEventKind,
        packet_number: u64,
        payload_len: u32,
        retransmissions: u32,
        pn_space: u8,
        cid_tag: u8,
    ) -> Self {
        Self {
            kind,
            packet_number,
            payload_len,
            retransmissions,
            pn_space,
            cid_tag,
        }
    }

    pub const fn with_pn_space(mut self, pn_space: u8) -> Self {
        self.pn_space = pn_space;
        self
    }

    pub const fn with_cid_tag(mut self, cid_tag: u8) -> Self {
        self.cid_tag = cid_tag;
        self
    }

    /// Encode the event into tap payload arguments.
    ///
    /// * `arg0` — lower 32 bits of the packet number
    /// * `arg1` — `[ kind | pn_space | cid_tag | payload_len | retransmissions ]`
    ///   * bits 29–31 store the event kind (0=Ack,1=Loss,2=KeepaliveTx,3=KeepaliveRx,4=CloseStart,5=CloseDraining,6=CloseRemote,7=Timeout)
    ///   * bits 26–28 store the packet number space identifier (3 bits)
    ///   * bits 18–25 store the connection identifier tag (8 bits)
    ///   * bits 8–17 store the payload length (saturated to 10 bits)
    ///   * bits 0–7 store the retransmission counter (saturated to 8 bits)
    pub fn encode_tap_args(&self) -> (u32, u32) {
        let arg0 = (self.packet_number & 0xFFFF_FFFF) as u32;
        let kind_bits = match self.kind {
            TransportEventKind::Ack => 0u32,
            TransportEventKind::Loss => 1u32,
            TransportEventKind::KeepaliveTx => 2u32,
            TransportEventKind::KeepaliveRx => 3u32,
            TransportEventKind::CloseStart => 4u32,
            TransportEventKind::CloseDraining => 5u32,
            TransportEventKind::CloseRemote => 6u32,
            TransportEventKind::Timeout => 7u32,
        };
        let pn_space = (self.pn_space as u32) & 0x7;
        let cid_tag = (self.cid_tag as u32) & 0xFF;
        let payload = self.payload_len.min(0x3FF) as u32;
        let retrans = self.retransmissions.min(0xFF) as u32;
        let arg1 =
            (kind_bits << 29) | (pn_space << 26) | (cid_tag << 18) | (payload << 8) | retrans;
        (arg0, arg1)
    }
}

/// Errors surfaced by transport operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportError {
    /// Backing medium rejected the frame (e.g. link down).
    Offline,
    /// Transport encountered a fatal error (driver reset, etc.).
    Failed,
}

/// Asynchronous transport interface with explicit Tx/Rx handles.
///
/// The trait uses GATs so that implementations can borrow buffers from the
/// surrounding environment without forcing allocations. Each method returns a
/// future; the crate purposefully avoids exposing a `poll_*` style API.
pub trait Transport {
    type Error: Into<TransportError>;
    type Tx<'a>: 'a
    where
        Self: 'a;
    type Rx<'a>: 'a
    where
        Self: 'a;
    type Send<'a>: Future<Output = Result<(), Self::Error>> + Unpin + 'a
    where
        Self: 'a;
    /// Future returned by [`recv`](Transport::recv).
    type Recv<'a>: Future<Output = Result<Payload<'a>, Self::Error>> + Unpin + 'a
    where
        Self: 'a;
    type Metrics: TransportMetrics;

    /// Open Tx/Rx handles bound to the lifetime of this transport reference.
    ///
    /// `local_role` is the role index of the endpoint attaching to the transport.
    /// Implementations can use this to route frames so that a role never
    /// receives the messages it emitted itself.
    ///
    /// `session_id` identifies the session for routing purposes. Implementations
    /// that multiplex multiple sessions over the same transport can use this to
    /// isolate message queues per session.
    fn open<'a>(&'a self, local_role: u8, session_id: u32) -> (Self::Tx<'a>, Self::Rx<'a>);

    /// Send a frame using the provided Tx handle.
    ///
    /// Transport implementations select the appropriate packet class
    /// (for example, pre-auth, handshake, or application-data) based on
    /// internal cryptographic
    /// state, not application-layer metadata.
    fn send<'a, 'f>(&'a self, tx: &'a mut Self::Tx<'a>, outgoing: Outgoing<'f>) -> Self::Send<'a>
    where
        'a: 'f;

    /// Receive a frame using the provided Rx handle.
    ///
    /// The future must resolve to a [`Payload`] view borrowed from the
    /// transport-managed receive slab. Borrowing ties the lifetime `'a` to the
    /// mutable borrow of `rx`, allowing higher layers such as [`crate::Endpoint`]
    /// to enforce that the view is released before the next receive.
    /// Implementations should store the current waker whenever the future parks
    /// so that hardware interrupts or other I/O notifications can wake the task
    /// directly instead of relying on polling loops.
    fn recv<'a>(&'a self, rx: &'a mut Self::Rx<'a>) -> Self::Recv<'a>;

    /// Requeue the most recent frame obtained from [`recv`](Transport::recv).
    ///
    /// Transports that support requeueing place the frame back onto their
    /// pending queue when higher layers cannot consume it.
    fn requeue<'a>(&'a self, rx: &'a mut Self::Rx<'a>);

    /// Drain transport-level telemetry events and forward them to the observer.
    ///
    /// Implementations invoke `emit` for each drained [`TransportEvent`].
    fn drain_events(&self, emit: &mut dyn FnMut(TransportEvent));

    /// Hint label for the most recently received payload.
    ///
    /// When a transport receives a frame that maps to a specific hibana message
    /// label (e.g., transport retry or version-negotiation control), it can return that label
    /// here to help route selection in passive observer mode.
    ///
    /// This must be non-blocking and must not perform I/O; it should only
    /// inspect transport state already available via `rx`.
    ///
    /// Implementations may treat hints as one-shot and clear them after returning
    /// a label, so repeated calls within the same offer yield `None`.
    ///
    fn recv_label_hint<'a>(&'a self, rx: &'a Self::Rx<'a>) -> Option<u8>;

    /// Provide transport-level metrics for routing decisions.
    ///
    /// Implementations supply latency estimates and queue depth information.
    fn metrics(&self) -> Self::Metrics;

    /// Apply pacing updates sourced from control-plane feedback.
    ///
    /// Implementations that expose pacing knobs apply the request explicitly.
    fn apply_pacing_update(&self, interval_us: u32, burst_bytes: u16);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::wire::Payload;
    use core::{
        cell::{Cell, UnsafeCell},
        future::{Future, ready},
        pin::Pin,
        task::{Context, Poll, RawWaker, RawWakerVTable, Waker},
    };

    #[derive(Default)]
    struct SharedState {
        waker: UnsafeCell<Option<Waker>>,
        ready: Cell<bool>,
    }

    impl SharedState {
        fn store_waker(&self, waker: &Waker) {
            unsafe {
                *self.waker.get() = Some(waker.clone());
            }
        }

        fn take_waker(&self) -> Option<Waker> {
            unsafe { (*self.waker.get()).take() }
        }

        fn set_ready(&self) {
            self.ready.set(true);
        }

        fn take_ready(&self) -> bool {
            self.ready.replace(false)
        }
    }

    struct RecvFuture<'a> {
        state: &'a SharedState,
        payload: Option<Payload<'a>>,
    }

    impl<'a> Future for RecvFuture<'a> {
        type Output = Result<Payload<'a>, TransportError>;

        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            self.state.store_waker(cx.waker());
            if self.state.take_ready() {
                let payload = self.payload.take().expect("payload only produced once");
                Poll::Ready(Ok(payload))
            } else {
                Poll::Pending
            }
        }
    }

    struct WakerAwareTransport {
        state: SharedState,
    }

    impl WakerAwareTransport {
        fn new() -> Self {
            Self {
                state: SharedState::default(),
            }
        }

        fn state(&self) -> &SharedState {
            &self.state
        }
    }

    impl Transport for WakerAwareTransport {
        type Error = TransportError;
        type Tx<'a>
            = ()
        where
            Self: 'a;
        type Rx<'a>
            = ()
        where
            Self: 'a;
        type Send<'a>
            = core::future::Ready<Result<(), Self::Error>>
        where
            Self: 'a;
        type Recv<'a>
            = RecvFuture<'a>
        where
            Self: 'a;
        type Metrics = ();

        fn open<'a>(&'a self, _local_role: u8, _session_id: u32) -> (Self::Tx<'a>, Self::Rx<'a>) {
            ((), ())
        }

        fn send<'a, 'f>(
            &'a self,
            _tx: &'a mut Self::Tx<'a>,
            _outgoing: Outgoing<'f>,
        ) -> Self::Send<'a>
        where
            'a: 'f,
        {
            ready(Ok(()))
        }

        fn recv<'a>(&'a self, _rx: &'a mut Self::Rx<'a>) -> Self::Recv<'a> {
            static PAYLOAD: [u8; 0] = [];
            let payload = Payload::new(&PAYLOAD);
            RecvFuture {
                state: &self.state,
                payload: Some(payload),
            }
        }

        fn requeue<'a>(&'a self, rx: &'a mut Self::Rx<'a>) {
            let _ = rx;
        }

        fn drain_events(&self, emit: &mut dyn FnMut(TransportEvent)) {
            let _ = emit;
        }

        fn recv_label_hint<'a>(&'a self, _rx: &'a Self::Rx<'a>) -> Option<u8> {
            None
        }

        fn metrics(&self) -> Self::Metrics {
            ()
        }

        fn apply_pacing_update(&self, interval_us: u32, burst_bytes: u16) {
            let _ = (interval_us, burst_bytes);
        }
    }

    unsafe fn flag_waker(flag: &Cell<bool>) -> Waker {
        unsafe fn clone(data: *const ()) -> RawWaker {
            RawWaker::new(data, &VTABLE)
        }

        unsafe fn wake(data: *const ()) {
            unsafe { (*(data as *const Cell<bool>)).set(true) };
        }

        unsafe fn wake_by_ref(data: *const ()) {
            unsafe { (*(data as *const Cell<bool>)).set(true) };
        }

        unsafe fn drop(_: *const ()) {}

        static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake_by_ref, drop);

        unsafe {
            Waker::from_raw(RawWaker::new(
                flag as *const Cell<bool> as *const (),
                &VTABLE,
            ))
        }
    }

    #[test]
    fn recv_future_records_waker_and_wakes() {
        let transport = WakerAwareTransport::new();
        let shared = transport.state();
        let mut rx = transport.open(0, 0).1;
        let mut future = transport.recv(&mut rx);

        assert!(shared.take_waker().is_none(), "no waker before polling");

        let wake_flag = Cell::new(false);
        let waker = unsafe { flag_waker(&wake_flag) };
        let mut cx = Context::from_waker(&waker);

        assert!(matches!(Pin::new(&mut future).poll(&mut cx), Poll::Pending));

        let stored = shared.take_waker().expect("future recorded waker");
        shared.set_ready();
        stored.wake();
        assert!(wake_flag.get(), "wake flag flipped");

        assert!(matches!(
            Pin::new(&mut future).poll(&mut cx),
            Poll::Ready(Ok(_))
        ));
    }
}

/// Transport context provider for resolver state access.
pub(crate) mod context;
/// Observability helpers for logical frame inspection.
pub(crate) mod trace;
/// Wire helpers: payload wrappers and serialization traits.
pub(crate) mod wire;
