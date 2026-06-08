use super::*;

use crate::observe::events;
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
fn route_arm_selection_event_decodes() {
    with_normalise_storage(|storage| {
        storage[0] = events::RouteArmSelection::with_causal(
            0,
            TapEvent::make_causal_key(4, 1),
            900,
            ((0x1234u32) << 16) | 0x7,
        )
        .with_arg2(0x8000_0000 | ((0x56u32) << 16) | 0x89);

        let events = delegation_trace(storage, 0, 1);
        assert_eq!(events.len(), 1);
        match events[0] {
            DelegationEvent::RouteArmSelection {
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
