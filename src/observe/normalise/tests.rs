use super::*;
use crate::observe::events;
use crate::transport::context::{self, ContextValue, PolicyAttrs};
use crate::transport::{
    TransportAlgorithm, TransportEvent, TransportEventKind, TransportEventMeta, TransportSnapshot,
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

const fn pack_endpoint_event(role: u8, lane: u8, label: u8, flags: u8) -> u32 {
    ((role as u32) << 24) | ((lane as u32) << 16) | ((label as u32) << 8) | (flags as u32)
}

const fn raw_event(ts: u32, id: u16, arg0: u32, arg1: u32) -> TapEvent {
    events::RawEvent::new(ts, id)
        .with_arg0(arg0)
        .with_arg1(arg1)
}

const fn endpoint_send_event(
    ts: u32,
    sid: u32,
    role: u8,
    lane: u8,
    label: u8,
    flags: u8,
) -> TapEvent {
    raw_event(
        ts,
        ids::ENDPOINT_SEND,
        sid,
        pack_endpoint_event(role, lane, label, flags),
    )
}

const fn endpoint_recv_event(
    ts: u32,
    sid: u32,
    role: u8,
    lane: u8,
    label: u8,
    flags: u8,
) -> TapEvent {
    raw_event(
        ts,
        ids::ENDPOINT_RECV,
        sid,
        pack_endpoint_event(role, lane, label, flags),
    )
}

const fn endpoint_control_event(
    ts: u32,
    sid: u32,
    role: u8,
    lane: u8,
    label: u8,
    flags: u8,
) -> TapEvent {
    raw_event(
        ts,
        ids::ENDPOINT_CONTROL,
        sid,
        pack_endpoint_event(role, lane, label, flags),
    )
}

const fn topology_ack_event(ts: u32, from: u8, to: u8, generation: u16, sid: u32) -> TapEvent {
    raw_event(
        ts,
        ids::TOPOLOGY_ACK,
        ((generation as u32) << 16) | ((to as u32) << 8) | (from as u32),
        sid,
    )
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
        storage[0] = endpoint_send_event(0, 9, 1, 2, 3, 0xAA);
        storage[1] = endpoint_recv_event(1, 9, 4, 5, 6, 0x55);
        storage[2] = endpoint_control_event(2, 9, 7, 8, 9, 0x10);

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
fn topology_ack_event_decodes() {
    with_normalise_storage(|storage| {
        storage[0] = topology_ack_event(0, 5, 0x12, 0x34, 900);

        let events = delegation_trace(storage, 0, 1);
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0],
            DelegationEvent::TopologyAck {
                sid: 900,
                from: 5,
                to: 0x12,
                generation: 0x34,
            }
        );
    });
}

#[test]
fn transport_trace_decodes_ack_and_loss() {
    with_normalise_storage(|storage| {
        let ack_event = TransportEvent::from_meta(
            TransportEventMeta::new(TransportEventKind::Ack)
                .packet_number(0xDEAD_BEEF)
                .payload_len(0x0155)
                .retransmissions(0x0012)
                .packet_number_space(2)
                .connection_id_tag(0x5A),
        );
        let loss_event = TransportEvent::from_meta(
            TransportEventMeta::new(TransportEventKind::Loss)
                .packet_number(0xFEED_FACE)
                .payload_len(0x01FF)
                .retransmissions(0x0055)
                .packet_number_space(1)
                .connection_id_tag(0x33),
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
        let mut attrs = PolicyAttrs::new();
        assert!(attrs.insert(context::core::LATENCY_US, ContextValue::from_u64(1500)));
        assert!(attrs.insert(context::core::QUEUE_DEPTH, ContextValue::from_u32(12)));
        assert!(attrs.insert(context::core::SRTT_US, ContextValue::from_u64(3200)));
        assert!(attrs.insert(
            context::core::CONGESTION_WINDOW,
            ContextValue::from_u64(64 * 1024),
        ));
        assert!(attrs.insert(
            context::core::IN_FLIGHT_BYTES,
            ContextValue::from_u64(32 * 1024),
        ));
        assert!(attrs.insert(context::core::RETRANSMISSIONS, ContextValue::from_u32(7),));
        assert!(attrs.insert(context::core::CONGESTION_MARKS, ContextValue::from_u32(3),));
        assert!(attrs.insert(
            context::core::PACING_INTERVAL_US,
            ContextValue::from_u64(500),
        ));
        assert!(attrs.insert(
            context::core::TRANSPORT_ALGORITHM,
            ContextValue::from_u32(1),
        ));
        let snapshot = TransportSnapshot::from_policy_attrs(&attrs);
        let payload = snapshot.encode_tap_metrics().expect("metrics encode");
        let (arg0, arg1) = payload.primary();
        storage[0] = events::TransportMetrics::new(0, arg0, arg1);
        if let Some((ext0, ext1)) = payload.extension() {
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
        let mut attrs = PolicyAttrs::new();
        assert!(attrs.insert(context::core::QUEUE_DEPTH, ContextValue::from_u32(4)));
        assert!(attrs.insert(context::core::SRTT_US, ContextValue::from_u64(6400)));
        assert!(attrs.insert(
            context::core::CONGESTION_WINDOW,
            ContextValue::from_u64(8 * 1024),
        ));
        assert!(attrs.insert(
            context::core::IN_FLIGHT_BYTES,
            ContextValue::from_u64(4 * 1024),
        ));
        assert!(attrs.insert(
            context::core::TRANSPORT_ALGORITHM,
            ContextValue::from_u32(2),
        ));
        let snapshot = TransportSnapshot::from_policy_attrs(&attrs);
        let payload = snapshot.encode_tap_metrics().expect("metrics encode");
        let (arg0, arg1) = payload.primary();
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
            storage[idx] = endpoint_send_event(idx as u32, 0x20, 0, 1, *label, 0);
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
        left[0] = endpoint_send_event(0, 0x30, 2, 1, 0x1, 0);
        right[0] = endpoint_send_event(0, 0x30, 3, 1, 0x2, 0);

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
            seq_storage[idx] = endpoint_send_event(idx as u32, 0x44, 0, lane, label, 0);
        }

        for (idx, &(lane, label)) in [(1u8, 0x40u8), (2, 0x50), (1, 0x41), (2, 0x51)]
            .iter()
            .enumerate()
        {
            interleaved_storage[idx] = endpoint_send_event(idx as u32, 0x44, 0, lane, label, 0);
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
