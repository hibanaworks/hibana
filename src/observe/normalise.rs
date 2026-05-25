//! Internal normalisation helpers for tap traces used by crate tests.
//!
//! # Unsafe Owner Contract
//!
//! The `cfg(test)` fixed traces in this module own `MaybeUninit` arrays with an
//! initialized-prefix invariant. Push writes the next slot before increasing
//! length; slice views are bounded by that length and borrow the trace, so they
//! cannot outlive or alias a later mutable push.
#[cfg(test)]
use crate::control::cap::mint::{EndpointResource, ResourceKind};
#[cfg(test)]
use crate::observe::core::TapEvent;
#[cfg(test)]
use crate::observe::ids;
#[cfg(test)]
use crate::observe::scope::{ScopeTrace, tap_scope};
#[cfg(test)]
use crate::runtime::consts::{RING_BUFFER_SIZE, RING_EVENTS};
#[cfg(test)]
use crate::transport::TransportAlgorithm;

#[cfg(test)]
use core::{mem::MaybeUninit, ops::Index, slice};

/// Boundary events emitted while delegation/topology control progresses.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DelegationEvent {
    Begin {
        sid: u32,
        shot_multi: bool,
        in_flight: u32,
    },
    Pick {
        sid: u32,
        policy: u32,
        shard: u32,
    },
    TopologyAck {
        sid: u32,
        from: u8,
        to: u8,
        generation: u16,
    },
    RouteDecision {
        sid: u32,
        lane: u8,
        scope: u16,
        arm: u16,
        decision: u8,
        range: u16,
        nest: u16,
    },
}

/// Events emitted by endpoints while they exchange frames.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EndpointEvent {
    Send {
        sid: u32,
        lane: u8,
        role: u8,
        label: u8,
        flags: u8,
        scope: Option<ScopeTrace>,
    },
    Recv {
        sid: u32,
        lane: u8,
        role: u8,
        label: u8,
        flags: u8,
        scope: Option<ScopeTrace>,
    },
    Control {
        sid: u32,
        lane: u8,
        role: u8,
        label: u8,
        flags: u8,
        scope: Option<ScopeTrace>,
    },
}

/// Transport-level telemetry emitted via tap events.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TransportTapEventKind {
    Ack,
    Loss,
    KeepaliveTx,
    KeepaliveRx,
    CloseStart,
    CloseDraining,
    CloseRemote,
}

/// Normalised representation of a transport telemetry tap event.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TransportTapEvent {
    kind: TransportTapEventKind,
    packet_number: u64,
    payload_len: u32,
    retransmissions: u32,
    pn_space: u8,
    cid_tag: u8,
}

#[cfg(test)]
impl TransportTapEvent {
    fn from_tap(event: TapEvent) -> Option<Self> {
        if event.id != ids::TRANSPORT_EVENT {
            return None;
        }
        let kind_bits = ((event.arg1 >> 29) & 0x7) as u8;
        let kind = match kind_bits {
            0 => TransportTapEventKind::Ack,
            1 => TransportTapEventKind::Loss,
            2 => TransportTapEventKind::KeepaliveTx,
            3 => TransportTapEventKind::KeepaliveRx,
            4 => TransportTapEventKind::CloseStart,
            5 => TransportTapEventKind::CloseDraining,
            6 => TransportTapEventKind::CloseRemote,
            _ => return None,
        };
        let pn_space = ((event.arg1 >> 26) & 0x7) as u8;
        let cid_tag = ((event.arg1 >> 18) & 0xFF) as u8;
        let payload_len = ((event.arg1 >> 8) & 0x3FF) as u32;
        let retransmissions = (event.arg1 & 0xFF) as u32;
        Some(Self {
            kind,
            packet_number: event.arg0 as u64,
            payload_len,
            retransmissions,
            pn_space,
            cid_tag,
        })
    }
}

/// Normalised transport congestion metrics emitted via tap events.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TransportMetricsTapEvent {
    algorithm: TransportAlgorithm,
    queue_depth: Option<u32>,
    srtt_us: Option<u64>,
    congestion_window: Option<u64>,
    in_flight_bytes: Option<u64>,
    retransmissions: Option<u32>,
    congestion_marks: Option<u32>,
    pacing_interval_us: Option<u64>,
}

#[cfg(test)]
impl TransportMetricsTapEvent {
    fn from_tap_pair(main: TapEvent, extension: Option<TapEvent>) -> Option<Self> {
        if main.id != ids::TRANSPORT_METRICS {
            return None;
        }
        let algo_id = ((main.arg0 >> 28) & 0xF) as u8;
        if algo_id == 0 {
            return None;
        }
        let algorithm = match algo_id {
            1 => TransportAlgorithm::Cubic,
            2 => TransportAlgorithm::Reno,
            other => TransportAlgorithm::Other(other),
        };
        let queue_bits = ((main.arg0 >> 16) & 0x0FFF) as u32;
        let queue_depth = if queue_bits == 0 {
            None
        } else {
            Some(queue_bits - 1)
        };
        let srtt_entry = (main.arg0 & 0xFFFF) as u32;
        let srtt_us = if srtt_entry == 0 {
            None
        } else {
            Some(((srtt_entry - 1) as u64) * 32)
        };
        let cwnd_entry = ((main.arg1 >> 16) & 0xFFFF) as u32;
        let congestion_window = if cwnd_entry == 0 {
            None
        } else {
            Some(((cwnd_entry - 1) as u64) * 1024)
        };
        let inflight_entry = (main.arg1 & 0xFFFF) as u32;
        let in_flight_bytes = if inflight_entry == 0 {
            None
        } else {
            Some(((inflight_entry - 1) as u64) * 1024)
        };
        let (retransmissions, congestion_marks, pacing_interval_us) = extension
            .filter(|event| event.id == ids::TRANSPORT_METRICS_EXT)
            .map(|event| {
                let retrans_entry = ((event.arg0 >> 16) & 0xFFFF) as u32;
                let cong_entry = (event.arg0 & 0xFFFF) as u32;
                let pacing_entry = event.arg1;
                let retransmissions = if retrans_entry == 0 {
                    None
                } else {
                    Some(retrans_entry - 1)
                };
                let congestion_marks = if cong_entry == 0 {
                    None
                } else {
                    Some(cong_entry - 1)
                };
                let pacing_interval_us = if pacing_entry == 0 {
                    None
                } else {
                    Some((pacing_entry - 1) as u64)
                };
                (retransmissions, congestion_marks, pacing_interval_us)
            })
            .unwrap_or((None, None, None));
        Some(Self {
            algorithm,
            queue_depth,
            srtt_us,
            congestion_window,
            in_flight_bytes,
            retransmissions,
            congestion_marks,
            pacing_interval_us,
        })
    }
}

/// Lifecycle stages for capability tokens.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CapEventStage {
    Mint,
    Claim,
    Exhaust,
}

/// Normalised capability lifecycle event (mint → claim → exhaust).
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CapEvent {
    kind_tag: u8,
    sid: u32,
    stage: CapEventStage,
    aux: u32,
}

#[cfg(test)]
fn resource_kind_name(tag: u8) -> &'static str {
    match tag {
        EndpointResource::TAG => EndpointResource::NAME,
        _ => "Unknown",
    }
}

#[cfg(test)]
impl CapEvent {
    fn kind_name(&self) -> &'static str {
        resource_kind_name(self.kind_tag)
    }
}

#[cfg(test)]
#[derive(Clone, Copy, Debug)]
struct FixedTrace<T: Copy, const N: usize> {
    len: usize,
    items: [MaybeUninit<T>; N],
}

#[cfg(test)]
impl<T: Copy, const N: usize> FixedTrace<T, N> {
    fn new() -> Self {
        Self {
            len: 0,
            items: /* SAFETY: the table owner tracks the initialized prefix and checks this slot before reading initialized storage. */ unsafe { MaybeUninit::<[MaybeUninit<T>; N]>::uninit().assume_init() },
        }
    }

    fn push(&mut self, value: T) {
        assert!(self.len < N, "fixed trace capacity exceeded");
        self.items[self.len].write(value);
        self.len += 1;
    }

    fn len(&self) -> usize {
        self.len
    }

    fn as_slice(&self) -> &[T] {
        /* SAFETY: the pointer and length are carved from one backing slice after bounds and alignment checks. */
        unsafe { slice::from_raw_parts(self.items.as_ptr() as *const T, self.len) }
    }

    fn as_mut_slice(&mut self) -> &mut [T] {
        /* SAFETY: the pointer and length are carved from one backing slice after bounds and alignment checks. */
        unsafe { slice::from_raw_parts_mut(self.items.as_mut_ptr() as *mut T, self.len) }
    }

    fn iter(&self) -> slice::Iter<'_, T> {
        self.as_slice().iter()
    }
}

#[cfg(test)]
impl<T: Copy + Ord, const N: usize> FixedTrace<T, N> {
    fn sort_unstable(&mut self) {
        self.as_mut_slice().sort_unstable();
    }
}

#[cfg(test)]
impl<T: Copy, const N: usize> Index<usize> for FixedTrace<T, N> {
    type Output = T;

    fn index(&self, index: usize) -> &Self::Output {
        &self.as_slice()[index]
    }
}

#[cfg(test)]
fn decode_cap_event(event: &TapEvent) -> Option<CapEvent> {
    let (stage, base) = if event.id >= ids::CAP_CLAIM_BASE && event.id < ids::CAP_EXHAUST_BASE {
        (CapEventStage::Claim, ids::CAP_CLAIM_BASE)
    } else if event.id >= ids::CAP_EXHAUST_BASE && event.id < ids::CAP_EXHAUST_BASE + 0x100 {
        (CapEventStage::Exhaust, ids::CAP_EXHAUST_BASE)
    } else if event.id >= ids::CAP_MINT_BASE && event.id < ids::CAP_CLAIM_BASE {
        (CapEventStage::Mint, ids::CAP_MINT_BASE)
    } else {
        return None;
    };

    let tag = (event.id - base) as u8;
    Some(CapEvent {
        kind_tag: tag,
        sid: event.arg0,
        stage,
        aux: event.arg1,
    })
}

/// Extract capability lifecycle events from a tap trace slice.
#[cfg(test)]
#[must_use]
fn cap_events(events: &[TapEvent]) -> FixedTrace<CapEvent, RING_EVENTS> {
    let mut out = FixedTrace::new();
    for event in events {
        if let Some(decoded) = decode_cap_event(event) {
            out.push(decoded);
        }
    }
    out
}

#[cfg(test)]
impl EndpointEvent {
    #[inline]
    fn sid(&self) -> u32 {
        match *self {
            EndpointEvent::Send { sid, .. }
            | EndpointEvent::Recv { sid, .. }
            | EndpointEvent::Control { sid, .. } => sid,
        }
    }

    #[inline]
    fn lane(&self) -> u8 {
        match *self {
            EndpointEvent::Send { lane, .. }
            | EndpointEvent::Recv { lane, .. }
            | EndpointEvent::Control { lane, .. } => lane,
        }
    }

    #[inline]
    fn role(&self) -> u8 {
        match *self {
            EndpointEvent::Send { role, .. }
            | EndpointEvent::Recv { role, .. }
            | EndpointEvent::Control { role, .. } => role,
        }
    }

    #[inline]
    fn label(&self) -> u8 {
        match *self {
            EndpointEvent::Send { label, .. }
            | EndpointEvent::Recv { label, .. }
            | EndpointEvent::Control { label, .. } => label,
        }
    }

    #[cfg(test)]
    #[inline]
    fn sort_key(&self) -> (u8, u32, u8, u8, u8, u8) {
        match *self {
            EndpointEvent::Send {
                sid,
                lane,
                role,
                label,
                flags,
                ..
            } => (0, sid, lane, role, label, flags),
            EndpointEvent::Recv {
                sid,
                lane,
                role,
                label,
                flags,
                ..
            } => (1, sid, lane, role, label, flags),
            EndpointEvent::Control {
                sid,
                lane,
                role,
                label,
                flags,
                ..
            } => (2, sid, lane, role, label, flags),
        }
    }
}

/// Normalises the tap range `[start, end)` into delegation/topology boundary events.
#[cfg(test)]
fn delegation_trace(
    storage: &[TapEvent],
    start: usize,
    end: usize,
) -> FixedTrace<DelegationEvent, RING_EVENTS> {
    let capacity = storage.len();
    debug_assert!(capacity == RING_EVENTS || capacity == RING_BUFFER_SIZE);
    let mut events = FixedTrace::new();
    let mut cursor = start;
    let mut current_sid: Option<u32> = None;
    while cursor < end {
        let raw = storage[cursor % capacity];
        match raw.id {
            ids::DELEG_BEGIN => {
                let sid = raw.arg0;
                current_sid = Some(sid);
                let shot_multi = ((raw.arg1 >> 31) & 0x1) != 0;
                let in_flight = raw.arg1 & 0x7FFF_FFFF;
                events.push(DelegationEvent::Begin {
                    sid,
                    shot_multi,
                    in_flight,
                });
            }
            ids::ROUTE_PICK => {
                let sid = current_sid.unwrap_or(0);
                events.push(DelegationEvent::Pick {
                    sid,
                    policy: raw.arg0,
                    shard: raw.arg1,
                });
            }
            ids::ROUTE_DECISION => {
                let scope = (raw.arg1 >> 16) as u16;
                let arm = (raw.arg1 & 0xFFFF) as u16;
                let (range, nest) = tap_scope(&raw)
                    .map(|trace| (trace.range, trace.nest))
                    .unwrap_or_else(|| {
                        let pack = raw.arg2;
                        (((pack >> 16) & 0xFFFF) as u16, (pack & 0xFFFF) as u16)
                    });
                events.push(DelegationEvent::RouteDecision {
                    sid: raw.arg0,
                    lane: raw.causal_role(),
                    scope,
                    arm,
                    decision: raw.causal_seq(),
                    range,
                    nest,
                });
            }
            ids::TOPOLOGY_ACK => {
                let sid = raw.arg1;
                current_sid = Some(sid);
                let encoded = raw.arg0;
                let from = (encoded & 0xFF) as u8;
                let to = ((encoded >> 8) & 0xFF) as u8;
                let generation = ((encoded >> 16) & 0xFFFF) as u16;
                events.push(DelegationEvent::TopologyAck {
                    sid,
                    from,
                    to,
                    generation,
                });
            }
            _ => {}
        }
        cursor += 1;
    }
    events
}

/// Normalises the tap range `[start, end)` into endpoint events, decoding
/// packed fields for easier comparison across seq/alt/par compositions.
#[cfg(test)]
fn endpoint_trace(
    storage: &[TapEvent],
    start: usize,
    end: usize,
) -> FixedTrace<EndpointEvent, RING_EVENTS> {
    let capacity = storage.len();
    debug_assert!(capacity == RING_EVENTS || capacity == RING_BUFFER_SIZE);
    let mut events = FixedTrace::new();
    let mut cursor = start;
    while cursor < end {
        let raw = storage[cursor % capacity];
        let packed = raw.arg1;
        let lane = ((packed >> 16) & 0xFF) as u8;
        let role = ((packed >> 24) & 0xFF) as u8;
        let label = ((packed >> 8) & 0xFF) as u8;
        let flags = (packed & 0xFF) as u8;
        let scope = tap_scope(&raw);
        match raw.id {
            ids::ENDPOINT_SEND => {
                events.push(EndpointEvent::Send {
                    sid: raw.arg0,
                    lane,
                    role,
                    label,
                    flags,
                    scope,
                });
            }
            ids::ENDPOINT_RECV => {
                events.push(EndpointEvent::Recv {
                    sid: raw.arg0,
                    lane,
                    role,
                    label,
                    flags,
                    scope,
                });
            }
            ids::ENDPOINT_CONTROL => {
                events.push(EndpointEvent::Control {
                    sid: raw.arg0,
                    lane,
                    role,
                    label,
                    flags,
                    scope,
                });
            }
            _ => {}
        }
        cursor += 1;
    }
    events
}

/// Collect transport telemetry events observed between `start` and `end`.
#[cfg(test)]
fn transport_trace(
    storage: &[TapEvent],
    start: usize,
    end: usize,
) -> FixedTrace<TransportTapEvent, RING_EVENTS> {
    let capacity = storage.len();
    debug_assert!(capacity == RING_EVENTS || capacity == RING_BUFFER_SIZE);
    let mut events = FixedTrace::new();
    let mut cursor = start;
    while cursor < end {
        let raw = storage[cursor % capacity];
        if let Some(event) = TransportTapEvent::from_tap(raw) {
            events.push(event);
        }
        cursor += 1;
    }
    events
}

/// Collect transport congestion metrics snapshots observed between `start` and `end`.
#[cfg(test)]
fn transport_metrics_trace(
    storage: &[TapEvent],
    start: usize,
    end: usize,
) -> FixedTrace<TransportMetricsTapEvent, RING_EVENTS> {
    let capacity = storage.len();
    debug_assert!(capacity == RING_EVENTS || capacity == RING_BUFFER_SIZE);
    let mut events = FixedTrace::new();
    let mut cursor = start;
    while cursor < end {
        let main = storage[cursor % capacity];
        if main.id == ids::TRANSPORT_METRICS {
            let mut extension = None;
            if cursor + 1 < end {
                let candidate = storage[(cursor + 1) % capacity];
                if candidate.id == ids::TRANSPORT_METRICS_EXT {
                    extension = Some(candidate);
                    cursor += 1;
                }
            }
            if let Some(event) = TransportMetricsTapEvent::from_tap_pair(main, extension) {
                events.push(event);
            }
        }
        cursor += 1;
    }
    events
}

#[cfg(test)]
mod tests;
