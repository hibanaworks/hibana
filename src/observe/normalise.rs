//! Internal normalisation helpers for tap traces used by crate tests.
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

/// Boundary events emitted by `Endpoint::reroute`.
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
    Splice {
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
            items: unsafe { MaybeUninit::<[MaybeUninit<T>; N]>::uninit().assume_init() },
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
        unsafe { slice::from_raw_parts(self.items.as_ptr() as *const T, self.len) }
    }

    fn as_mut_slice(&mut self) -> &mut [T] {
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

/// Normalises the tap range `[start, end)` into delegation boundary events.
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
            ids::DELEG_SPLICE => {
                let sid = raw.arg1;
                current_sid = Some(sid);
                let encoded = raw.arg0;
                let from = (encoded & 0xFF) as u8;
                let to = ((encoded >> 8) & 0xFF) as u8;
                let generation = ((encoded >> 16) & 0xFFFF) as u16;
                events.push(DelegationEvent::Splice {
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
mod tests {
    use super::*;
    use crate::observe::events;
    use crate::transport::{
        TransportAlgorithm, TransportEvent, TransportEventKind, TransportSnapshot,
        TransportSnapshotParts,
    };
    use core::cell::UnsafeCell;
    use std::thread_local;

    thread_local! {
        static NORMALISE_PRIMARY: UnsafeCell<[TapEvent; RING_EVENTS]> =
            const { UnsafeCell::new([TapEvent::zero(); RING_EVENTS]) };
        static NORMALISE_SECONDARY: UnsafeCell<[TapEvent; RING_EVENTS]> =
            const { UnsafeCell::new([TapEvent::zero(); RING_EVENTS]) };
    }

    fn with_normalise_storage<R>(f: impl FnOnce(&'static mut [TapEvent; RING_EVENTS]) -> R) -> R {
        NORMALISE_PRIMARY.with(|storage| {
            let storage = unsafe { &mut *storage.get() };
            storage.fill(TapEvent::zero());
            f(storage)
        })
    }

    fn with_normalise_storage_pair<R>(
        f: impl FnOnce(&'static mut [TapEvent; RING_EVENTS], &'static mut [TapEvent; RING_EVENTS]) -> R,
    ) -> R {
        NORMALISE_PRIMARY.with(|left| {
            NORMALISE_SECONDARY.with(|right| {
                let left = unsafe { &mut *left.get() };
                let right = unsafe { &mut *right.get() };
                left.fill(TapEvent::zero());
                right.fill(TapEvent::zero());
                f(left, right)
            })
        })
    }

    #[test]
    fn cap_events_decode_lifecycle_stages() {
        let sid = 42;
        let events = [
            events::RawEvent::new(1, crate::observe::cap_mint::<EndpointResource>())
                .with_arg0(sid)
                .with_arg1(0),
            events::RawEvent::new(2, crate::observe::cap_claim::<EndpointResource>())
                .with_arg0(sid)
                .with_arg1(0),
            events::RawEvent::new(3, crate::observe::cap_exhaust::<EndpointResource>())
                .with_arg0(sid)
                .with_arg1(0),
        ];

        let caps = cap_events(&events);
        assert_eq!(caps.len(), 3);
        assert_eq!(caps[0].stage, CapEventStage::Mint);
        assert_eq!(caps[0].kind_name(), "EndpointResource");
        assert_eq!(caps[0].sid, sid);
        assert_eq!(caps[1].stage, CapEventStage::Claim);
        assert_eq!(caps[2].stage, CapEventStage::Exhaust);
    }

    #[test]
    fn endpoint_trace_decodes_sends_and_recvs() {
        with_normalise_storage(|storage| {
            storage[0] = events::EndpointSend::new(0, 9, events::EndpointSend::pack(1, 2, 3, 0xAA));
            storage[1] = events::EndpointRecv::new(1, 9, events::EndpointRecv::pack(4, 5, 6, 0x55));
            storage[2] =
                events::EndpointControl::new(2, 9, events::EndpointControl::pack(7, 8, 9, 0x10));

            let events = endpoint_trace(storage, 0, 3);
            assert_eq!(events.len(), 3);
            assert_eq!(
                events[0],
                EndpointEvent::Send {
                    sid: 9,
                    lane: 2,
                    role: 1,
                    label: 3,
                    flags: 0xAA,
                    scope: None,
                }
            );
            assert_eq!(
                events[1],
                EndpointEvent::Recv {
                    sid: 9,
                    lane: 5,
                    role: 4,
                    label: 6,
                    flags: 0x55,
                    scope: None,
                }
            );
            assert_eq!(
                events[2],
                EndpointEvent::Control {
                    sid: 9,
                    lane: 8,
                    role: 7,
                    label: 9,
                    flags: 0x10,
                    scope: None,
                }
            );
        });
    }

    #[test]
    fn route_decision_event_decodes() {
        with_normalise_storage(|storage| {
            storage[0] = events::RouteDecision::with_causal(
                0,
                TapEvent::make_causal_key(4, 1),
                900,
                ((0x1234u32) << 16) | 0x7,
            )
            .with_arg2(0x8000_0000 | ((0x56u32) << 16) | 0x89);

            let events = delegation_trace(storage, 0, 1);
            assert_eq!(events.len(), 1);
            match events[0] {
                DelegationEvent::RouteDecision {
                    sid,
                    lane,
                    scope,
                    arm,
                    decision,
                    range,
                    nest,
                } => {
                    assert_eq!(sid, 900);
                    assert_eq!(lane, 4);
                    assert_eq!(scope, 0x1234);
                    assert_eq!(arm, 0x7);
                    assert_eq!(decision, 1);
                    assert_eq!(range, 0x56);
                    assert_eq!(nest, 0x89);
                }
                other => panic!("unexpected event: {other:?}"),
            }
        });
    }

    #[test]
    fn transport_trace_decodes_ack_and_loss() {
        with_normalise_storage(|storage| {
            let ack_event = TransportEvent::new_with_metadata(
                TransportEventKind::Ack,
                0xDEAD_BEEF,
                0x0155,
                0x0012,
                2,
                0x5A,
            );
            let loss_event = TransportEvent::new_with_metadata(
                TransportEventKind::Loss,
                0xFEED_FACE,
                0x01FF,
                0x0055,
                1,
                0x33,
            );
            let (ack_arg0, ack_arg1) = ack_event.encode_tap_args();
            let (loss_arg0, loss_arg1) = loss_event.encode_tap_args();
            storage[0] = events::TransportEvent::new(0, ack_arg0, ack_arg1);
            storage[1] = events::TransportEvent::new(1, loss_arg0, loss_arg1);

            let events = transport_trace(storage, 0, 2);
            assert_eq!(events.len(), 2);
            assert_eq!(events[0].kind, TransportTapEventKind::Ack);
            assert_eq!(events[0].packet_number, 0xDEAD_BEEF);
            assert_eq!(events[0].payload_len, 0x0155);
            assert_eq!(events[0].retransmissions, 0x0012);
            assert_eq!(events[0].pn_space, 2);
            assert_eq!(events[0].cid_tag, 0x5A);

            assert_eq!(events[1].kind, TransportTapEventKind::Loss);
            assert_eq!(events[1].packet_number, 0xFEED_FACE);
            assert_eq!(events[1].payload_len, 0x01FF);
            assert_eq!(events[1].retransmissions, 0x0055);
            assert_eq!(events[1].pn_space, 1);
            assert_eq!(events[1].cid_tag, 0x33);
        });
    }

    #[test]
    fn transport_metrics_trace_decodes_snapshot() {
        with_normalise_storage(|storage| {
            let snapshot = TransportSnapshot::from_parts(TransportSnapshotParts {
                latency_us: Some(1500),
                queue_depth: Some(12),
                srtt_us: Some(3200),
                congestion_window: Some(64 * 1024),
                in_flight_bytes: Some(32 * 1024),
                retransmissions: Some(7),
                congestion_marks: Some(3),
                pacing_interval_us: Some(500),
                algorithm: Some(TransportAlgorithm::Cubic),
                ..TransportSnapshotParts::new()
            });
            let payload = snapshot.encode_tap_metrics().expect("metrics encode");
            let (arg0, arg1) = payload.primary;
            storage[0] = events::TransportMetrics::new(0, arg0, arg1);
            if let Some((ext0, ext1)) = payload.extension {
                storage[1] = events::TransportMetricsExt::new(1, ext0, ext1);
            }

            let events = transport_metrics_trace(storage, 0, 2);
            assert_eq!(events.len(), 1);
            let event = events[0];
            assert_eq!(event.algorithm, TransportAlgorithm::Cubic);
            assert_eq!(event.queue_depth, Some(12));
            assert_eq!(event.srtt_us, Some(3200));
            assert_eq!(event.congestion_window, Some(64 * 1024));
            assert_eq!(event.in_flight_bytes, Some(32 * 1024));
            assert_eq!(event.retransmissions, Some(7));
            assert_eq!(event.congestion_marks, Some(3));
            assert_eq!(event.pacing_interval_us, Some(500));
        });
    }

    #[test]
    fn transport_metrics_trace_handles_missing_extension() {
        with_normalise_storage(|storage| {
            let snapshot = TransportSnapshot::from_parts(TransportSnapshotParts {
                queue_depth: Some(4),
                srtt_us: Some(6400),
                congestion_window: Some(8 * 1024),
                in_flight_bytes: Some(4 * 1024),
                algorithm: Some(TransportAlgorithm::Reno),
                ..TransportSnapshotParts::new()
            });
            let payload = snapshot.encode_tap_metrics().expect("metrics encode");
            let (arg0, arg1) = payload.primary;
            storage[0] = events::TransportMetrics::new(0, arg0, arg1);

            let events = transport_metrics_trace(storage, 0, 1);
            assert_eq!(events.len(), 1);
            let event = events[0];
            assert_eq!(event.algorithm, TransportAlgorithm::Reno);
            assert_eq!(event.retransmissions, None);
            assert_eq!(event.congestion_marks, None);
            assert_eq!(event.pacing_interval_us, None);
        });
    }

    #[test]
    fn endpoint_seq_events_preserve_order() {
        with_normalise_storage(|storage| {
            for (idx, label) in [10u8, 11, 12].iter().enumerate() {
                storage[idx] = events::EndpointSend::new(
                    idx as u32,
                    0x20,
                    events::EndpointSend::pack(0, 1, *label, 0),
                );
            }

            let events = endpoint_trace(storage, 0, 3);
            let mut labels = FixedTrace::<u8, RING_EVENTS>::new();
            for event in events.iter() {
                labels.push(event.label());
            }
            assert_eq!(labels.as_slice(), &[10, 11, 12]);

            labels.sort_unstable();
            assert_eq!(labels.as_slice(), &[10, 11, 12]);
        });
    }

    #[test]
    fn endpoint_alt_arms_remain_disjoint() {
        with_normalise_storage_pair(|left, right| {
            left[0] = events::EndpointSend::new(0, 0x30, events::EndpointSend::pack(2, 1, 0x1, 0));
            right[0] = events::EndpointSend::new(0, 0x30, events::EndpointSend::pack(3, 1, 0x2, 0));

            let left_events = endpoint_trace(left, 0, 1);
            let right_events = endpoint_trace(right, 0, 1);

            let mut combined = FixedTrace::<(u32, u8, u8, u8, u8), 2>::new();
            for event in left_events.iter().chain(right_events.iter()) {
                combined.push({
                    let kind = match event {
                        EndpointEvent::Send { .. } => 0u8,
                        EndpointEvent::Recv { .. } => 1u8,
                        EndpointEvent::Control { .. } => 2u8,
                    };
                    (event.sid(), event.lane(), event.role(), event.label(), kind)
                });
            }
            combined.sort_unstable();

            assert_eq!(combined.len(), 2);
            assert!(combined[0].3 != combined[1].3);
        });
    }

    #[test]
    fn endpoint_par_traces_align_after_sorting() {
        with_normalise_storage_pair(|seq_storage, interleaved_storage| {
            for (idx, &(lane, label)) in [(1u8, 0x40u8), (1, 0x41), (2, 0x50), (2, 0x51)]
                .iter()
                .enumerate()
            {
                seq_storage[idx] = events::EndpointSend::new(
                    idx as u32,
                    0x44,
                    events::EndpointSend::pack(0, lane, label, 0),
                );
            }

            for (idx, &(lane, label)) in [(1u8, 0x40u8), (2, 0x50), (1, 0x41), (2, 0x51)]
                .iter()
                .enumerate()
            {
                interleaved_storage[idx] = events::EndpointSend::new(
                    idx as u32,
                    0x44,
                    events::EndpointSend::pack(0, lane, label, 0),
                );
            }

            let seq_events = endpoint_trace(seq_storage, 0, 4);
            let interleaved_events = endpoint_trace(interleaved_storage, 0, 4);

            let mut seq_keys = FixedTrace::<(u8, u32, u8, u8, u8, u8), RING_EVENTS>::new();
            for event in seq_events.iter() {
                seq_keys.push(event.sort_key());
            }
            let mut interleaved_keys = FixedTrace::<(u8, u32, u8, u8, u8, u8), RING_EVENTS>::new();
            for event in interleaved_events.iter() {
                interleaved_keys.push(event.sort_key());
            }

            seq_keys.sort_unstable();
            interleaved_keys.sort_unstable();

            assert_eq!(seq_keys.as_slice(), interleaved_keys.as_slice());
        });
    }
}
