//! Transport abstraction bridging Hibana frames onto concrete mediums.
//!
//! Implementations are expected to integrate with external async runtimes via
//! explicit `poll_*` methods. The transport owns whatever pending state and
//! waker bookkeeping it needs inside its `Tx` / `Rx` handles or shared state.
//!
//! Receive buffers must be exposed as borrowed views. The rendezvous layer
//! provides a slab (see [`crate::runtime::config::Config::slab`]) that transports can pin
//! behind their `Rx` handle so [`Transport::poll_recv`] yields payload views borrowed
//! from that storage. This keeps the runtime allocation-free while allowing
//! DMA/SHM backed zero-copy paths.
//!
//! Implementations also bridge device interrupts to the task waker stored by
//! their pending send/recv state. When a poll parks it must record the current
//! [`core::task::Waker`] so the interrupt handler can call `wake_by_ref`
//! instead of relying on polling loops.

use core::{
    ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign, Not},
    task::{Context, Poll},
};

use crate::{
    eff::EffIndex,
    transport::wire::{CodecError, Payload, WireEncode, WirePayload, require_exact_len},
};

/// Choreography-facing message / branch identity.
///
/// This is intentionally crate-private. Application code expresses logical
/// labels through `g::Msg<L, P, K>` and observes them through `RouteBranch`.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct LogicalLabel(u8);

impl LogicalLabel {
    #[inline]
    pub(crate) const fn new(raw: u8) -> Self {
        Self(raw)
    }

    #[inline]
    pub(crate) const fn raw(self) -> u8 {
        self.0
    }
}

/// Transport-facing discriminator for a projected local frame.
///
/// Application choreography labels remain logical branch/message identities.
/// `FrameLabel` is the compact demux value consumed by transports and bindings.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FrameLabel(u8);

impl FrameLabel {
    #[inline]
    pub const fn new(raw: u8) -> Self {
        Self(raw)
    }

    #[inline]
    pub const fn raw(self) -> u8 {
        self.0
    }
}

/// Fixed mask over the complete `FrameLabel` domain.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct FrameLabelMask {
    low: u128,
    high: u128,
}

impl FrameLabelMask {
    pub(crate) const EMPTY: Self = Self { low: 0, high: 0 };

    #[inline]
    pub(crate) const fn from_frame_label(frame_label: u8) -> Self {
        if frame_label < u128::BITS as u8 {
            Self {
                low: 1u128 << frame_label,
                high: 0,
            }
        } else {
            Self {
                low: 0,
                high: 1u128 << ((frame_label - u128::BITS as u8) as u32),
            }
        }
    }

    #[inline]
    pub(crate) const fn is_empty(self) -> bool {
        self.low == 0 && self.high == 0
    }

    #[inline]
    pub(crate) const fn contains_frame_label(self, frame_label: u8) -> bool {
        if frame_label < u128::BITS as u8 {
            (self.low & (1u128 << frame_label)) != 0
        } else {
            (self.high & (1u128 << ((frame_label - u128::BITS as u8) as u32))) != 0
        }
    }

    #[inline]
    pub(crate) const fn intersects(self, other: Self) -> bool {
        (self.low & other.low) != 0 || (self.high & other.high) != 0
    }

    #[inline]
    pub(crate) const fn without(self, other: Self) -> Self {
        Self {
            low: self.low & !other.low,
            high: self.high & !other.high,
        }
    }

    #[inline]
    pub(crate) fn insert_frame_label(&mut self, frame_label: u8) -> bool {
        let before = *self;
        *self |= Self::from_frame_label(frame_label);
        before != *self
    }

    #[inline]
    pub(crate) fn remove_frame_label(&mut self, frame_label: u8) {
        *self = self.without(Self::from_frame_label(frame_label));
    }

    #[inline]
    pub(crate) const fn singleton_frame_label(self) -> Option<u8> {
        if self.low != 0 {
            if self.high != 0 || (self.low & (self.low - 1)) != 0 {
                return None;
            }
            return Some(self.low.trailing_zeros() as u8);
        }
        if self.high == 0 || (self.high & (self.high - 1)) != 0 {
            return None;
        }
        Some((self.high.trailing_zeros() as u8) + u128::BITS as u8)
    }

    #[inline]
    pub(crate) fn take_matching<F>(&mut self, mut matches: F) -> Option<u8>
    where
        F: FnMut(u8) -> bool,
    {
        let mut remaining = self.low;
        while remaining != 0 {
            let frame_label = remaining.trailing_zeros() as u8;
            if matches(frame_label) {
                self.remove_frame_label(frame_label);
                return Some(frame_label);
            }
            remaining &= remaining - 1;
        }

        let mut remaining = self.high;
        while remaining != 0 {
            let frame_label = (remaining.trailing_zeros() as u8) + u128::BITS as u8;
            if matches(frame_label) {
                self.remove_frame_label(frame_label);
                return Some(frame_label);
            }
            remaining &= remaining - 1;
        }
        None
    }

    #[cfg(test)]
    pub(crate) fn has_matching<F>(self, mut matches: F) -> bool
    where
        F: FnMut(u8) -> bool,
    {
        let mut remaining = self.low;
        while remaining != 0 {
            let frame_label = remaining.trailing_zeros() as u8;
            if matches(frame_label) {
                return true;
            }
            remaining &= remaining - 1;
        }

        let mut remaining = self.high;
        while remaining != 0 {
            let frame_label = (remaining.trailing_zeros() as u8) + u128::BITS as u8;
            if matches(frame_label) {
                return true;
            }
            remaining &= remaining - 1;
        }
        false
    }
}

impl BitOr for FrameLabelMask {
    type Output = Self;

    #[inline]
    fn bitor(self, rhs: Self) -> Self::Output {
        Self {
            low: self.low | rhs.low,
            high: self.high | rhs.high,
        }
    }
}

impl BitOrAssign for FrameLabelMask {
    #[inline]
    fn bitor_assign(&mut self, rhs: Self) {
        self.low |= rhs.low;
        self.high |= rhs.high;
    }
}

impl BitAnd for FrameLabelMask {
    type Output = Self;

    #[inline]
    fn bitand(self, rhs: Self) -> Self::Output {
        Self {
            low: self.low & rhs.low,
            high: self.high & rhs.high,
        }
    }
}

impl BitAndAssign for FrameLabelMask {
    #[inline]
    fn bitand_assign(&mut self, rhs: Self) {
        self.low &= rhs.low;
        self.high &= rhs.high;
    }
}

impl Not for FrameLabelMask {
    type Output = Self;

    #[inline]
    fn not(self) -> Self::Output {
        Self {
            low: !self.low,
            high: !self.high,
        }
    }
}

/// Congestion control algorithm observed by a transport.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TransportAlgorithm {
    Cubic,
    Reno,
    Other(u8),
}

const SNAPSHOT_LATENCY_US: u16 = 1 << 0;
const SNAPSHOT_QUEUE_DEPTH: u16 = 1 << 1;
const SNAPSHOT_PACING_INTERVAL_US: u16 = 1 << 2;
const SNAPSHOT_CONGESTION_MARKS: u16 = 1 << 3;
const SNAPSHOT_RETRANSMISSIONS: u16 = 1 << 4;
const SNAPSHOT_PTO_COUNT: u16 = 1 << 5;
const SNAPSHOT_SRTT_US: u16 = 1 << 6;
const SNAPSHOT_LATEST_ACK_PN: u16 = 1 << 7;
const SNAPSHOT_CONGESTION_WINDOW: u16 = 1 << 8;
const SNAPSHOT_IN_FLIGHT_BYTES: u16 = 1 << 9;
const SNAPSHOT_ALGORITHM: u16 = 1 << 10;

/// Internal snapshot of transport-level observations supplied to routing policies.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TransportSnapshot {
    present: u16,
    latency_us: u64,
    queue_depth: u32,
    pacing_interval_us: u64,
    congestion_marks: u32,
    retransmissions: u32,
    pto_count: u32,
    srtt_us: u64,
    latest_ack_pn: u64,
    congestion_window: u64,
    in_flight_bytes: u64,
    algorithm: TransportAlgorithm,
}

impl Default for TransportSnapshot {
    fn default() -> Self {
        Self {
            present: 0,
            latency_us: 0,
            queue_depth: 0,
            pacing_interval_us: 0,
            congestion_marks: 0,
            retransmissions: 0,
            pto_count: 0,
            srtt_us: 0,
            latest_ack_pn: 0,
            congestion_window: 0,
            in_flight_bytes: 0,
            algorithm: TransportAlgorithm::Other(0),
        }
    }
}

impl TransportSnapshot {
    /// Construct a transport snapshot from packed policy attributes.
    pub(crate) const fn from_policy_attrs(attrs: &context::PolicyAttrs) -> Self {
        Self {
            present: 0,
            latency_us: 0,
            queue_depth: 0,
            pacing_interval_us: 0,
            congestion_marks: 0,
            retransmissions: 0,
            pto_count: 0,
            srtt_us: 0,
            latest_ack_pn: 0,
            congestion_window: 0,
            in_flight_bytes: 0,
            algorithm: TransportAlgorithm::Other(0),
        }
        .set_latency_us(match attrs.get(context::core::LATENCY_US) {
            Some(value) => Some(value.as_u64()),
            None => None,
        })
        .set_queue_depth(match attrs.get(context::core::QUEUE_DEPTH) {
            Some(value) => Some(value.as_u32()),
            None => None,
        })
        .set_pacing_interval(match attrs.get(context::core::PACING_INTERVAL_US) {
            Some(value) => Some(value.as_u64()),
            None => None,
        })
        .set_congestion_marks(match attrs.get(context::core::CONGESTION_MARKS) {
            Some(value) => Some(value.as_u32()),
            None => None,
        })
        .set_retransmissions(match attrs.get(context::core::RETRANSMISSIONS) {
            Some(value) => Some(value.as_u32()),
            None => None,
        })
        .set_pto_count(match attrs.get(context::core::PTO_COUNT) {
            Some(value) => Some(value.as_u32()),
            None => None,
        })
        .set_srtt(match attrs.get(context::core::SRTT_US) {
            Some(value) => Some(value.as_u64()),
            None => None,
        })
        .set_latest_ack(match attrs.get(context::core::LATEST_ACK_PN) {
            Some(value) => Some(value.as_u64()),
            None => None,
        })
        .set_congestion_window(match attrs.get(context::core::CONGESTION_WINDOW) {
            Some(value) => Some(value.as_u64()),
            None => None,
        })
        .set_in_flight(match attrs.get(context::core::IN_FLIGHT_BYTES) {
            Some(value) => Some(value.as_u64()),
            None => None,
        })
        .set_algorithm(decode_transport_algorithm(
            attrs.get(context::core::TRANSPORT_ALGORITHM),
        ))
    }

    #[inline]
    pub const fn queue_depth(&self) -> Option<u32> {
        if (self.present & SNAPSHOT_QUEUE_DEPTH) != 0 {
            Some(self.queue_depth)
        } else {
            None
        }
    }

    #[inline]
    pub const fn pacing_interval_us(&self) -> Option<u64> {
        if (self.present & SNAPSHOT_PACING_INTERVAL_US) != 0 {
            Some(self.pacing_interval_us)
        } else {
            None
        }
    }

    #[inline]
    pub const fn congestion_marks(&self) -> Option<u32> {
        if (self.present & SNAPSHOT_CONGESTION_MARKS) != 0 {
            Some(self.congestion_marks)
        } else {
            None
        }
    }

    #[inline]
    pub const fn retransmissions(&self) -> Option<u32> {
        if (self.present & SNAPSHOT_RETRANSMISSIONS) != 0 {
            Some(self.retransmissions)
        } else {
            None
        }
    }

    #[inline]
    pub const fn srtt_us(&self) -> Option<u64> {
        if (self.present & SNAPSHOT_SRTT_US) != 0 {
            Some(self.srtt_us)
        } else {
            None
        }
    }

    #[inline]
    pub const fn congestion_window(&self) -> Option<u64> {
        if (self.present & SNAPSHOT_CONGESTION_WINDOW) != 0 {
            Some(self.congestion_window)
        } else {
            None
        }
    }

    #[inline]
    pub const fn in_flight_bytes(&self) -> Option<u64> {
        if (self.present & SNAPSHOT_IN_FLIGHT_BYTES) != 0 {
            Some(self.in_flight_bytes)
        } else {
            None
        }
    }

    #[inline]
    pub const fn algorithm(&self) -> Option<TransportAlgorithm> {
        if (self.present & SNAPSHOT_ALGORITHM) != 0 {
            Some(self.algorithm)
        } else {
            None
        }
    }

    #[inline]
    const fn set_latency_us(mut self, latency_us: Option<u64>) -> Self {
        match latency_us {
            Some(value) => {
                self.present |= SNAPSHOT_LATENCY_US;
                self.latency_us = value;
            }
            None => {
                self.present &= !SNAPSHOT_LATENCY_US;
                self.latency_us = 0;
            }
        }
        self
    }

    #[inline]
    const fn set_queue_depth(mut self, queue_depth: Option<u32>) -> Self {
        match queue_depth {
            Some(value) => {
                self.present |= SNAPSHOT_QUEUE_DEPTH;
                self.queue_depth = value;
            }
            None => {
                self.present &= !SNAPSHOT_QUEUE_DEPTH;
                self.queue_depth = 0;
            }
        }
        self
    }

    /// Attach congestion mark statistics (ECN-CE or equivalent) to the snapshot.
    const fn set_congestion_marks(mut self, congestion_marks: Option<u32>) -> Self {
        match congestion_marks {
            Some(value) => {
                self.present |= SNAPSHOT_CONGESTION_MARKS;
                self.congestion_marks = value;
            }
            None => {
                self.present &= !SNAPSHOT_CONGESTION_MARKS;
                self.congestion_marks = 0;
            }
        }
        self
    }

    /// Attach a pacing interval recommendation (microseconds between packets).
    const fn set_pacing_interval(mut self, pacing_interval_us: Option<u64>) -> Self {
        match pacing_interval_us {
            Some(value) => {
                self.present |= SNAPSHOT_PACING_INTERVAL_US;
                self.pacing_interval_us = value;
            }
            None => {
                self.present &= !SNAPSHOT_PACING_INTERVAL_US;
                self.pacing_interval_us = 0;
            }
        }
        self
    }

    /// Attach retransmission statistics to the snapshot.
    const fn set_retransmissions(mut self, retransmissions: Option<u32>) -> Self {
        match retransmissions {
            Some(value) => {
                self.present |= SNAPSHOT_RETRANSMISSIONS;
                self.retransmissions = value;
            }
            None => {
                self.present &= !SNAPSHOT_RETRANSMISSIONS;
                self.retransmissions = 0;
            }
        }
        self
    }

    /// Attach PTO count statistics to the snapshot.
    const fn set_pto_count(mut self, pto_count: Option<u32>) -> Self {
        match pto_count {
            Some(value) => {
                self.present |= SNAPSHOT_PTO_COUNT;
                self.pto_count = value;
            }
            None => {
                self.present &= !SNAPSHOT_PTO_COUNT;
                self.pto_count = 0;
            }
        }
        self
    }

    /// Attach an RTT estimate (Smoothed RTT in microseconds).
    const fn set_srtt(mut self, srtt_us: Option<u64>) -> Self {
        match srtt_us {
            Some(value) => {
                self.present |= SNAPSHOT_SRTT_US;
                self.srtt_us = value;
            }
            None => {
                self.present &= !SNAPSHOT_SRTT_US;
                self.srtt_us = 0;
            }
        }
        self
    }

    /// Attach the most recent acknowledged packet number.
    const fn set_latest_ack(mut self, latest_ack_pn: Option<u64>) -> Self {
        match latest_ack_pn {
            Some(value) => {
                self.present |= SNAPSHOT_LATEST_ACK_PN;
                self.latest_ack_pn = value;
            }
            None => {
                self.present &= !SNAPSHOT_LATEST_ACK_PN;
                self.latest_ack_pn = 0;
            }
        }
        self
    }

    /// Attach a congestion window estimate (bytes) to the snapshot.
    const fn set_congestion_window(mut self, congestion_window: Option<u64>) -> Self {
        match congestion_window {
            Some(value) => {
                self.present |= SNAPSHOT_CONGESTION_WINDOW;
                self.congestion_window = value;
            }
            None => {
                self.present &= !SNAPSHOT_CONGESTION_WINDOW;
                self.congestion_window = 0;
            }
        }
        self
    }

    /// Attach the number of bytes currently considered in flight.
    const fn set_in_flight(mut self, in_flight_bytes: Option<u64>) -> Self {
        match in_flight_bytes {
            Some(value) => {
                self.present |= SNAPSHOT_IN_FLIGHT_BYTES;
                self.in_flight_bytes = value;
            }
            None => {
                self.present &= !SNAPSHOT_IN_FLIGHT_BYTES;
                self.in_flight_bytes = 0;
            }
        }
        self
    }

    /// Attach the congestion control algorithm affecting this snapshot.
    const fn set_algorithm(mut self, algorithm: Option<TransportAlgorithm>) -> Self {
        match algorithm {
            Some(value) => {
                self.present |= SNAPSHOT_ALGORITHM;
                self.algorithm = value;
            }
            None => {
                self.present &= !SNAPSHOT_ALGORITHM;
                self.algorithm = TransportAlgorithm::Other(0);
            }
        }
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
        let algorithm = self.algorithm()?;
        let algo_bits = match algorithm {
            TransportAlgorithm::Cubic => 1u32,
            TransportAlgorithm::Reno => 2u32,
            TransportAlgorithm::Other(code) => (code as u32).min(0xF).max(1),
        };
        let queue_depth = self
            .queue_depth()
            .map(|value| value.min(0x0FFE) + 1)
            .unwrap_or(0);
        let srtt_units = self
            .srtt_us()
            .map(|value| ((value / 32).min(0xFFFE) as u32) + 1)
            .unwrap_or(0);
        let congestion_window = self
            .congestion_window()
            .map(|bytes| ((bytes / 1024).min(0xFFFE) as u32) + 1)
            .unwrap_or(0);
        let in_flight = self
            .in_flight_bytes()
            .map(|bytes| ((bytes / 1024).min(0xFFFE) as u32) + 1)
            .unwrap_or(0);
        let arg0 = (algo_bits << 28) | (queue_depth << 16) | srtt_units;
        let arg1 = (congestion_window << 16) | in_flight;
        let extension_needed = self.retransmissions().is_some()
            || self.congestion_marks().is_some()
            || self.pacing_interval_us().is_some();
        let extension = if extension_needed {
            let retransmissions = self
                .retransmissions()
                .map(|value| value.min(0xFFFE) + 1)
                .unwrap_or(0);
            let congestion_marks = self
                .congestion_marks()
                .map(|value| value.min(0xFFFE) + 1)
                .unwrap_or(0);
            let pacing_interval = self
                .pacing_interval_us()
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

#[inline]
const fn decode_transport_algorithm(
    value: Option<context::ContextValue>,
) -> Option<TransportAlgorithm> {
    match value {
        Some(value) => match value.as_u32() {
            1 => Some(TransportAlgorithm::Cubic),
            2 => Some(TransportAlgorithm::Reno),
            raw if raw >= 0x100 => Some(TransportAlgorithm::Other((raw - 0x100) as u8)),
            raw => Some(TransportAlgorithm::Other(raw as u8)),
        },
        None => None,
    }
}

/// Packed tap payload emitted for transport metrics sampling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TransportMetricsTapPayload {
    primary: (u32, u32),
    extension: Option<(u32, u32)>,
}

impl TransportMetricsTapPayload {
    #[inline]
    pub(crate) const fn primary(&self) -> (u32, u32) {
        self.primary
    }

    #[inline]
    pub(crate) const fn extension(&self) -> Option<(u32, u32)> {
        self.extension
    }
}

/// Metrics facade returned by transports to feed routing SLO checks.
pub trait TransportMetrics {
    /// Convert the current readings into packed policy attributes.
    fn attrs(&self) -> context::PolicyAttrs;
}

impl TransportMetrics for () {
    fn attrs(&self) -> context::PolicyAttrs {
        context::PolicyAttrs::EMPTY
    }
}

/// Direction of a send operation from the local role's perspective.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LocalDirection {
    /// Sending to a peer over the transport.
    Send,
    /// Local-only self-send that must not hit the wire.
    Local,
}

/// Transport-owned metadata for an outgoing payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SendMeta {
    /// Effect index (stable identifier for the choreography step).
    pub eff_index: EffIndex,
    /// Application/choreography logical label.
    pub logical_label: LogicalLabel,
    /// Transport/binding demux discriminator.
    pub frame_label: FrameLabel,
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
    pub const fn is_local(&self) -> bool {
        matches!(self.direction, LocalDirection::Local)
    }
}

/// Transport-owned outgoing frame.
#[derive(Clone, Copy, Debug)]
pub struct Outgoing<'f> {
    pub(crate) meta: SendMeta,
    pub(crate) payload: Payload<'f>,
}

impl<'f> Outgoing<'f> {
    #[inline]
    pub const fn frame_label(&self) -> FrameLabel {
        self.meta.frame_label
    }

    #[inline]
    pub const fn peer(&self) -> u8 {
        self.meta.peer
    }

    #[inline]
    pub const fn lane(&self) -> u8 {
        self.meta.lane
    }

    #[inline]
    pub const fn is_control(&self) -> bool {
        self.meta.is_control
    }

    #[inline]
    pub const fn is_send(&self) -> bool {
        self.meta.is_send()
    }

    #[inline]
    pub const fn is_local(&self) -> bool {
        self.meta.is_local()
    }

    #[inline]
    pub const fn payload(&self) -> Payload<'f> {
        self.payload
    }
}

/// Transport-level telemetry event taxonomy.
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
        require_exact_len(bytes.len(), 1, "transport event kind payload length")?;
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
    kind: TransportEventKind,
    packet_number: u64,
    payload_len: u32,
    retransmissions: u32,
    /// Packet number space identifier (transport-defined).
    pn_space: u8,
    /// Truncated tag identifying the relevant connection identifier (transport-defined).
    cid_tag: u8,
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
        require_exact_len(bytes.len(), LEN, "transport event payload length")?;
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

    pub(crate) const fn new_with_metadata(
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

    #[inline]
    pub const fn kind(&self) -> TransportEventKind {
        self.kind
    }

    #[inline]
    pub const fn packet_number(&self) -> u64 {
        self.packet_number
    }

    #[inline]
    pub const fn payload_len(&self) -> u32 {
        self.payload_len
    }

    #[inline]
    pub const fn retransmissions(&self) -> u32 {
        self.retransmissions
    }

    #[inline]
    pub const fn pn_space(&self) -> u8 {
        self.pn_space
    }

    #[inline]
    pub const fn cid_tag(&self) -> u8 {
        self.cid_tag
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
/// surrounding environment without forcing allocations. Pending I/O state stays
/// in transport-owned handles instead of leaking transport future types into
/// higher layers.
pub trait Transport {
    type Error: Into<TransportError>;
    type Tx<'a>: 'a
    where
        Self: 'a;
    type Rx<'a>: 'a
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

    /// Progress a send operation using the provided Tx handle.
    ///
    /// Transport implementations select the appropriate packet class
    /// (for example, pre-auth, handshake, or application-data) based on
    /// internal cryptographic
    /// state, not application-layer metadata.
    fn poll_send<'a, 'f>(
        &'a self,
        tx: &'a mut Self::Tx<'a>,
        outgoing: Outgoing<'f>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>>
    where
        'a: 'f;

    /// Cancel any transport-owned pending send state bound to `tx`.
    ///
    /// Public endpoint send futures are affine and may be dropped after
    /// `poll_send` parks. When a transport stages frame state inside `Tx` or
    /// transport-owned shared state before returning `Poll::Pending`, it must
    /// discard that staged state here so that a retry cannot flush the
    /// cancelled payload.
    fn cancel_send<'a>(&'a self, tx: &'a mut Self::Tx<'a>);

    /// Progress a receive operation using the provided Rx handle.
    ///
    /// The returned [`Payload`] view is borrowed from the transport-managed
    /// receive slab. Borrowing ties the lifetime `'a` to the mutable borrow of
    /// `rx`, allowing higher layers such as [`crate::Endpoint`] to enforce that
    /// the view is released before the next receive. Implementations should
    /// store the current waker whenever the poll parks so that hardware
    /// interrupts or other I/O notifications can wake the task directly instead
    /// of relying on polling loops.
    fn poll_recv<'a>(
        &'a self,
        rx: &'a mut Self::Rx<'a>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Payload<'a>, Self::Error>>;

    /// Requeue the most recent frame obtained from [`poll_recv`](Transport::poll_recv).
    ///
    /// Transports that support requeueing place the frame back onto their
    /// pending queue when higher layers cannot consume it.
    fn requeue<'a>(&'a self, rx: &'a mut Self::Rx<'a>);

    /// Drain transport-level telemetry events and forward them to the observer.
    ///
    /// Implementations invoke `emit` for each drained [`TransportEvent`].
    fn drain_events(&self, emit: &mut dyn FnMut(TransportEvent));

    /// Hint frame label for the most recently received payload.
    ///
    /// When a transport receives a frame that maps to a specific hibana message
    /// frame label, it can return that discriminator here to help descriptor-
    /// checked passive route observation.
    ///
    /// This must be non-blocking and must not perform I/O; it should only
    /// inspect transport state already available via `rx`.
    ///
    /// Implementations may treat hints as one-shot and clear them after returning
    /// a frame label, so repeated calls within the same offer yield `None`.
    ///
    fn recv_frame_hint<'a>(&'a self, rx: &'a Self::Rx<'a>) -> Option<FrameLabel>;

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

    struct WakerAwareTransport {
        state: SharedState,
    }

    #[test]
    fn transport_event_fixed_decoders_reject_trailing_bytes() {
        assert_eq!(
            TransportEventKind::decode_payload(Payload::new(&[0])),
            Ok(TransportEventKind::Ack)
        );
        assert_eq!(
            TransportEventKind::decode_payload(Payload::new(&[0, 0])),
            Err(CodecError::Invalid("transport event kind payload length"))
        );

        let event = TransportEvent::new_with_metadata(
            TransportEventKind::Loss,
            0x0102_0304_0506_0708,
            0x1122_3344,
            0x5566_7788,
            3,
            4,
        );
        let mut encoded = [0u8; 20];
        assert_eq!(event.encode_into(&mut encoded[..19]), Ok(19));
        assert_eq!(
            TransportEvent::decode_payload(Payload::new(&encoded[..19])),
            Ok(event)
        );
        assert_eq!(
            TransportEvent::decode_payload(Payload::new(&encoded)),
            Err(CodecError::Invalid("transport event payload length"))
        );
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
        type Metrics = ();

        fn open<'a>(&'a self, _local_role: u8, _session_id: u32) -> (Self::Tx<'a>, Self::Rx<'a>) {
            ((), ())
        }

        fn poll_send<'a, 'f>(
            &'a self,
            _tx: &'a mut Self::Tx<'a>,
            _outgoing: Outgoing<'f>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>>
        where
            'a: 'f,
        {
            Poll::Ready(Ok(()))
        }

        fn poll_recv<'a>(
            &'a self,
            _rx: &'a mut Self::Rx<'a>,
            cx: &mut Context<'_>,
        ) -> Poll<Result<Payload<'a>, Self::Error>> {
            static PAYLOAD: [u8; 0] = [];
            self.state.store_waker(cx.waker());
            if self.state.take_ready() {
                Poll::Ready(Ok(Payload::new(&PAYLOAD)))
            } else {
                Poll::Pending
            }
        }

        fn cancel_send<'a>(&'a self, _tx: &'a mut Self::Tx<'a>) {}

        fn requeue<'a>(&'a self, rx: &'a mut Self::Rx<'a>) {
            let _ = rx;
        }

        fn drain_events(&self, emit: &mut dyn FnMut(TransportEvent)) {
            let _ = emit;
        }

        fn recv_frame_hint<'a>(&'a self, _rx: &'a Self::Rx<'a>) -> Option<FrameLabel> {
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

        assert!(shared.take_waker().is_none(), "no waker before polling");

        let wake_flag = Cell::new(false);
        let waker = unsafe { flag_waker(&wake_flag) };
        let mut cx = Context::from_waker(&waker);

        assert!(matches!(
            transport.poll_recv(&mut rx, &mut cx),
            Poll::Pending
        ));

        let stored = shared.take_waker().expect("future recorded waker");
        shared.set_ready();
        stored.wake();
        assert!(wake_flag.get(), "wake flag flipped");

        assert!(matches!(
            transport.poll_recv(&mut rx, &mut cx),
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
