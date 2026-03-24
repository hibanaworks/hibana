use super::{
    Action, PolicyMode, hash_policy_input, hash_tap_event, hash_transport_snapshot,
    policy_mode_tag, replay_transport_inputs, replay_transport_presence, run_with, slot_tag,
    verdict_arm, verdict_reason, verdict_tag,
};
use crate::{
    control::cap::mint::CapsMask,
    observe::{
        core::{TapEvent, TapRing, install_ring, uninstall_ring},
        events::RawEvent,
        ids,
    },
    runtime::consts::RING_EVENTS,
    transport::TransportSnapshot,
};
use std::{
    boxed::Box,
    collections::BTreeMap,
    sync::{Mutex, OnceLock},
    vec,
};

const SLOT: super::vm::Slot = super::vm::Slot::Route;
const FUEL_MAX: u16 = 64;
const MEM_LEN: usize = 128;
const CODE_V1: [u8; 2] = [super::ops::instr::NOP, super::ops::instr::HALT];
const CODE_V2: [u8; 3] = [super::ops::instr::ACT_ABORT, 0x01, 0x00];
const POLICY_REPLAY_EVENT_ID: u16 = 0x040A;
const POLICY_REPLAY_INPUT0_ID: u16 = 0x040B;
const POLICY_REPLAY_INPUT1_ID: u16 = 0x040C;
const POLICY_REPLAY_TRANSPORT0_ID: u16 = 0x040D;
const POLICY_REPLAY_TRANSPORT1_ID: u16 = 0x040E;
const POLICY_REPLAY_EVENT_EXT_ID: u16 = 0x040F;

fn leak_tap_storage() -> &'static mut [TapEvent; RING_EVENTS] {
    let storage: Box<[TapEvent]> = vec![TapEvent::default(); RING_EVENTS].into_boxed_slice();
    let storage: Box<[TapEvent; RING_EVENTS]> = storage.try_into().expect("ring events length");
    Box::leak(storage)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct AuditVerdict {
    verdict_meta: u32,
    reason: u32,
    fuel_used: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ReplayPending {
    core: Option<(u32, u32, u32)>,
    ext: Option<(u32, u32, u32)>,
    replay_event: Option<(u32, u16, u32)>,
    replay_event_ext: Option<(u32, u32, u16)>,
    replay_input0: Option<(u32, u32, u32)>,
    replay_input1: Option<u32>,
    replay_transport0: Option<(u32, u32, u32)>,
    replay_transport1: Option<(u32, u8)>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct DigestState {
    active_digest: Option<u32>,
    standby_digest: Option<u32>,
    last_good_digest: Option<u32>,
}

impl ReplayPending {
    #[inline]
    fn is_empty(&self) -> bool {
        self.core.is_none()
            && self.ext.is_none()
            && self.replay_event.is_none()
            && self.replay_event_ext.is_none()
            && self.replay_input0.is_none()
            && self.replay_input1.is_none()
            && self.replay_transport0.is_none()
            && self.replay_transport1.is_none()
    }

    #[inline]
    fn clear(&mut self) {
        *self = Self::default();
    }
}

fn policy_replay_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn install_test_ring() -> (&'static TapRing<'static>, Option<&'static TapRing<'static>>) {
    let storage = leak_tap_storage();
    let ring = Box::leak(Box::new(TapRing::from_storage(storage)));
    let previous = unsafe { install_ring(assume_static_ring(ring)) };
    (ring, previous)
}

fn restore_ring(ring: &'static TapRing<'static>, previous: Option<&'static TapRing<'static>>) {
    let _ = uninstall_ring(ring);
    if let Some(prev) = previous {
        let _ = install_ring(prev);
    }
}

unsafe fn assume_static_ring(ring: &TapRing<'_>) -> &'static TapRing<'static> {
    unsafe { &*(ring as *const TapRing<'_>).cast::<TapRing<'static>>() }
}

fn replay_digests(ring: &TapRing<'_>, slot: super::vm::Slot, cursor: &mut usize) -> DigestState {
    let mut replay = DigestState::default();
    for event in ring.events_since(cursor, |event| Some(event)) {
        if event.arg0 != slot_tag(slot) as u32 {
            continue;
        }
        match event.id {
            ids::POLICY_COMMIT => {
                if let Some(current) = replay.active_digest {
                    replay.last_good_digest = Some(current);
                }
                replay.active_digest = Some(event.arg2);
                replay.standby_digest = None;
            }
            ids::POLICY_ROLLBACK => {
                replay.standby_digest = replay.active_digest;
                replay.active_digest = Some(event.arg2);
                replay.last_good_digest = Some(event.arg2);
            }
            _ => {}
        }
    }
    replay
}

fn replay_catalog() -> BTreeMap<u32, &'static [u8]> {
    let mut catalog = BTreeMap::new();
    let _ = catalog.insert(super::verifier::compute_hash(&CODE_V1), &CODE_V1[..]);
    let _ = catalog.insert(super::verifier::compute_hash(&CODE_V2), &CODE_V2[..]);
    catalog
}

fn decode_slot_mode(raw: u32) -> Result<(super::vm::Slot, PolicyMode), &'static str> {
    let slot = match ((raw >> 24) & 0xFF) as u8 {
        0 => super::vm::Slot::Forward,
        1 => super::vm::Slot::EndpointRx,
        2 => super::vm::Slot::EndpointTx,
        3 => super::vm::Slot::Rendezvous,
        4 => super::vm::Slot::Route,
        _ => return Err("invalid slot tag"),
    };
    let mode = match ((raw >> 16) & 0xFF) as u8 {
        0 => PolicyMode::Shadow,
        1 => PolicyMode::Enforce,
        _ => return Err("invalid mode tag"),
    };
    Ok((slot, mode))
}

fn replay_transport_snapshot(values: [u32; 4], presence: u8) -> TransportSnapshot {
    let latency = if (presence & (1 << 0)) != 0 {
        Some(values[0] as u64)
    } else {
        None
    };
    let queue_depth = if (presence & (1 << 1)) != 0 {
        Some(values[1])
    } else {
        None
    };
    let congestion_marks = if (presence & (1 << 2)) != 0 {
        Some(values[2])
    } else {
        None
    };
    let retransmissions = if (presence & (1 << 3)) != 0 {
        Some(values[3])
    } else {
        None
    };
    TransportSnapshot::new(latency, queue_depth)
        .with_congestion_marks(congestion_marks)
        .with_retransmissions(retransmissions)
}

fn verdict_from_action(action: Action, fuel_used: u32) -> AuditVerdict {
    let verdict = action.verdict();
    AuditVerdict {
        verdict_meta: ((verdict_tag(verdict) as u32) << 24) | ((verdict_arm(verdict) as u32) << 16),
        reason: verdict_reason(verdict) as u32,
        fuel_used,
    }
}

fn execute_policy_verdict(
    catalog: &BTreeMap<u32, &'static [u8]>,
    policy_digest: u32,
    slot: super::vm::Slot,
    mode: PolicyMode,
    event: &TapEvent,
    policy_input: [u32; 4],
    transport_snapshot: TransportSnapshot,
) -> Result<AuditVerdict, &'static str> {
    let code = *catalog
        .get(&policy_digest)
        .ok_or("unknown policy digest for replay")?;
    let mut scratch = [0u8; MEM_LEN];
    let machine = super::host::Machine::with_mem(code, &mut scratch, MEM_LEN, FUEL_MAX)
        .map_err(|_| "machine init failed")?;
    let mut host_slots = super::host::HostSlots::new();
    host_slots
        .install(slot, machine)
        .map_err(|_| "host install failed")?;
    host_slots.set_policy_mode(slot, mode);
    let action = run_with(
        &host_slots,
        slot,
        event,
        CapsMask::allow_all(),
        None,
        None,
        |ctx| {
            ctx.set_policy_input(policy_input);
            ctx.set_transport_snapshot(transport_snapshot);
        },
    );
    Ok(verdict_from_action(
        action,
        host_slots.last_fuel_used(slot) as u32,
    ))
}

fn push_policy_audit_tuple(
    ring: &TapRing<'_>,
    ts: u32,
    slot: super::vm::Slot,
    mode: PolicyMode,
    policy_digest: u32,
    event_id: u16,
    event_arg0: u32,
    event_arg1: u32,
    policy_input: [u32; 4],
    transport_snapshot: TransportSnapshot,
    verdict: AuditVerdict,
) {
    let replay_transport = replay_transport_inputs(transport_snapshot);
    let replay_transport_presence = replay_transport_presence(transport_snapshot);
    let event = RawEvent::new(ts, event_id)
        .with_arg0(event_arg0)
        .with_arg1(event_arg1);
    ring.push(
        RawEvent::new(ts, ids::POLICY_AUDIT)
            .with_arg0(policy_digest)
            .with_arg1(hash_tap_event(&event))
            .with_arg2(hash_policy_input(policy_input)),
    );
    ring.push(
        RawEvent::new(ts, ids::POLICY_AUDIT_EXT)
            .with_arg0(0)
            .with_arg1(hash_transport_snapshot(transport_snapshot))
            .with_arg2(((slot_tag(slot) as u32) << 24) | ((policy_mode_tag(mode) as u32) << 16)),
    );
    ring.push(
        RawEvent::new(ts, POLICY_REPLAY_EVENT_ID)
            .with_arg0(event.ts)
            .with_arg1(event.id as u32)
            .with_arg2(event.arg0),
    );
    ring.push(
        RawEvent::new(ts, POLICY_REPLAY_EVENT_EXT_ID)
            .with_arg0(event.arg1)
            .with_arg1(event.arg2)
            .with_arg2(event.causal_key as u32),
    );
    ring.push(
        RawEvent::new(ts, POLICY_REPLAY_INPUT0_ID)
            .with_arg0(policy_input[0])
            .with_arg1(policy_input[1])
            .with_arg2(policy_input[2]),
    );
    ring.push(
        RawEvent::new(ts, POLICY_REPLAY_INPUT1_ID)
            .with_arg0(policy_input[3])
            .with_arg1(0),
    );
    ring.push(
        RawEvent::new(ts, POLICY_REPLAY_TRANSPORT0_ID)
            .with_arg0(replay_transport[0])
            .with_arg1(replay_transport[1])
            .with_arg2(replay_transport[2]),
    );
    ring.push(
        RawEvent::new(ts, POLICY_REPLAY_TRANSPORT1_ID)
            .with_arg0(replay_transport[3])
            .with_arg1(replay_transport_presence as u32),
    );
    ring.push(
        RawEvent::new(ts, ids::POLICY_AUDIT_RESULT)
            .with_arg0(verdict.verdict_meta)
            .with_arg1(verdict.reason)
            .with_arg2(verdict.fuel_used),
    );
}

fn replay_verdict_from_audit(
    ring: &TapRing<'_>,
    cursor: &mut usize,
    catalog: &BTreeMap<u32, &'static [u8]>,
) -> Result<usize, &'static str> {
    let mut pending = ReplayPending::default();
    let mut rows = 0usize;
    let mut error: Option<&'static str> = None;

    for event in ring.events_since(cursor, |event| Some(event)) {
        if error.is_some() {
            break;
        }
        match event.id {
            ids::POLICY_AUDIT => {
                if !pending.is_empty() {
                    error = Some("incomplete audit tuple");
                    break;
                }
                pending.core = Some((event.arg0, event.arg1, event.arg2));
            }
            ids::POLICY_AUDIT_EXT => {
                if pending.core.is_none() {
                    error = Some("incomplete audit tuple");
                    break;
                }
                pending.ext = Some((event.arg0, event.arg1, event.arg2));
            }
            POLICY_REPLAY_EVENT_ID => {
                if pending.core.is_none() || pending.ext.is_none() {
                    error = Some("incomplete audit tuple");
                    break;
                }
                let event_id = match u16::try_from(event.arg1) {
                    Ok(value) => value,
                    Err(_) => {
                        error = Some("invalid replay event id");
                        break;
                    }
                };
                pending.replay_event = Some((event.arg0, event_id, event.arg2));
            }
            POLICY_REPLAY_EVENT_EXT_ID => {
                if pending.core.is_none() || pending.ext.is_none() || pending.replay_event.is_none()
                {
                    error = Some("incomplete audit tuple");
                    break;
                }
                let causal_key = match u16::try_from(event.arg2) {
                    Ok(value) => value,
                    Err(_) => {
                        error = Some("invalid replay causal key");
                        break;
                    }
                };
                pending.replay_event_ext = Some((event.arg0, event.arg1, causal_key));
            }
            POLICY_REPLAY_INPUT0_ID => {
                if pending.core.is_none() {
                    error = Some("incomplete audit tuple");
                    break;
                }
                pending.replay_input0 = Some((event.arg0, event.arg1, event.arg2));
            }
            POLICY_REPLAY_INPUT1_ID => {
                if pending.core.is_none() {
                    error = Some("incomplete audit tuple");
                    break;
                }
                pending.replay_input1 = Some(event.arg0);
            }
            POLICY_REPLAY_TRANSPORT0_ID => {
                if pending.core.is_none() {
                    error = Some("incomplete audit tuple");
                    break;
                }
                pending.replay_transport0 = Some((event.arg0, event.arg1, event.arg2));
            }
            POLICY_REPLAY_TRANSPORT1_ID => {
                if pending.core.is_none() {
                    error = Some("incomplete audit tuple");
                    break;
                }
                let presence = match u8::try_from(event.arg1) {
                    Ok(value) => value,
                    Err(_) => {
                        error = Some("invalid transport presence");
                        break;
                    }
                };
                pending.replay_transport1 = Some((event.arg0, presence));
            }
            ids::POLICY_AUDIT_DEFER => {
                let source = ((event.arg0 >> 24) & 0xFF) as u8;
                if source > 2 {
                    error = Some("invalid defer source");
                    break;
                }
                let reason = ((event.arg2 >> 16) & 0xFFFF) as u16;
                if reason != 1 && reason != 2 {
                    error = Some("invalid defer reason");
                    break;
                }
            }
            ids::POLICY_AUDIT_RESULT => {
                let Some((policy_digest, event_hash, signals_input_hash)) = pending.core.take()
                else {
                    error = Some("incomplete audit tuple");
                    break;
                };
                let Some((_, transport_snapshot_hash, slot_mode)) = pending.ext.take() else {
                    error = Some("incomplete audit tuple");
                    break;
                };
                let Some((event_ts, event_id, event_arg0)) = pending.replay_event.take() else {
                    error = Some("incomplete audit tuple");
                    break;
                };
                let Some((event_arg1, event_arg2, event_causal_key)) =
                    pending.replay_event_ext.take()
                else {
                    error = Some("incomplete audit tuple");
                    break;
                };
                let Some((input0, input1, input2)) = pending.replay_input0.take() else {
                    error = Some("incomplete audit tuple");
                    break;
                };
                let Some(input3) = pending.replay_input1.take() else {
                    error = Some("incomplete audit tuple");
                    break;
                };
                let Some((latency, queue, congestion)) = pending.replay_transport0.take() else {
                    error = Some("incomplete audit tuple");
                    break;
                };
                let Some((retry, transport_presence)) = pending.replay_transport1.take() else {
                    error = Some("incomplete audit tuple");
                    break;
                };

                let policy_event = RawEvent::new(event_ts, event_id)
                    .with_causal_key(event_causal_key)
                    .with_arg0(event_arg0)
                    .with_arg1(event_arg1)
                    .with_arg2(event_arg2);
                if hash_tap_event(&policy_event) != event_hash {
                    error = Some("event hash mismatch");
                    break;
                }
                let policy_input = [input0, input1, input2, input3];
                if hash_policy_input(policy_input) != signals_input_hash {
                    error = Some("input hash mismatch");
                    break;
                }
                let transport_values = [latency, queue, congestion, retry];
                let transport_snapshot =
                    replay_transport_snapshot(transport_values, transport_presence);
                if hash_transport_snapshot(transport_snapshot) != transport_snapshot_hash {
                    error = Some("transport hash mismatch");
                    break;
                }
                let (slot, mode) = match decode_slot_mode(slot_mode) {
                    Ok(value) => value,
                    Err(err) => {
                        error = Some(err);
                        break;
                    }
                };
                let expected = match execute_policy_verdict(
                    catalog,
                    policy_digest,
                    slot,
                    mode,
                    &policy_event,
                    policy_input,
                    transport_snapshot,
                ) {
                    Ok(verdict) => verdict,
                    Err(err) => {
                        error = Some(err);
                        break;
                    }
                };
                let logged = AuditVerdict {
                    verdict_meta: event.arg0,
                    reason: event.arg1,
                    fuel_used: event.arg2,
                };
                if expected != logged {
                    error = Some("verdict replay mismatch");
                    break;
                }

                rows = rows.saturating_add(1);
                pending.clear();
            }
            _ => {}
        }
    }

    if let Some(err) = error {
        return Err(err);
    }
    if !pending.is_empty() {
        return Err("incomplete audit tuple");
    }
    Ok(rows)
}

#[test]
fn replay_from_audit_log_tracks_digest_transitions() {
    let _guard = policy_replay_test_lock()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let (ring, previous) = install_test_ring();
    let mut cursor = ring.head();
    let digest_v1 = super::verifier::compute_hash(&CODE_V1);
    let digest_v2 = super::verifier::compute_hash(&CODE_V2);

    ring.push(
        RawEvent::new(1, ids::POLICY_COMMIT)
            .with_arg0(slot_tag(SLOT) as u32)
            .with_arg1(1)
            .with_arg2(digest_v1),
    );
    ring.push(
        RawEvent::new(2, ids::POLICY_COMMIT)
            .with_arg0(slot_tag(SLOT) as u32)
            .with_arg1(2)
            .with_arg2(digest_v2),
    );
    ring.push(
        RawEvent::new(3, ids::POLICY_ROLLBACK)
            .with_arg0(slot_tag(SLOT) as u32)
            .with_arg1(1)
            .with_arg2(digest_v1),
    );

    let replay = replay_digests(ring, SLOT, &mut cursor);
    let expected = DigestState {
        active_digest: Some(digest_v1),
        standby_digest: Some(digest_v2),
        last_good_digest: Some(digest_v1),
    };
    assert_eq!(replay, expected);

    restore_ring(ring, previous);
}

#[test]
fn replay_from_audit_log_recomputes_same_verdict() {
    let _guard = policy_replay_test_lock()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let (ring, previous) = install_test_ring();
    let mut cursor = ring.head();
    let catalog = replay_catalog();

    let slot = super::vm::Slot::Route;
    let mode = PolicyMode::Enforce;
    let policy_digest = super::verifier::compute_hash(&CODE_V1);
    let policy_input = [1, 0, 0, 0];
    let transport_snapshot = TransportSnapshot::new(Some(11), Some(2))
        .with_congestion_marks(Some(1))
        .with_retransmissions(Some(0));

    let event_one = RawEvent::new(1, ids::ROUTE_DECISION)
        .with_arg0(1)
        .with_arg1(42);
    let expected_one = execute_policy_verdict(
        &catalog,
        policy_digest,
        slot,
        mode,
        &event_one,
        policy_input,
        transport_snapshot,
    )
    .expect("expected verdict one");
    push_policy_audit_tuple(
        ring,
        1,
        slot,
        mode,
        policy_digest,
        ids::ROUTE_DECISION,
        1,
        42,
        policy_input,
        transport_snapshot,
        expected_one,
    );

    let event_two = RawEvent::new(2, ids::ROUTE_DECISION)
        .with_arg0(1)
        .with_arg1(42);
    let expected_two = execute_policy_verdict(
        &catalog,
        policy_digest,
        slot,
        mode,
        &event_two,
        policy_input,
        transport_snapshot,
    )
    .expect("expected verdict two");
    push_policy_audit_tuple(
        ring,
        2,
        slot,
        mode,
        policy_digest,
        ids::ROUTE_DECISION,
        1,
        42,
        policy_input,
        transport_snapshot,
        expected_two,
    );

    let rows =
        replay_verdict_from_audit(ring, &mut cursor, &catalog).expect("verdict replay must match");
    assert_eq!(rows, 2);

    restore_ring(ring, previous);
}

#[test]
fn replay_from_audit_log_detects_verdict_divergence() {
    let _guard = policy_replay_test_lock()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let (ring, previous) = install_test_ring();
    let mut cursor = ring.head();
    let catalog = replay_catalog();

    let slot = super::vm::Slot::Route;
    let mode = PolicyMode::Enforce;
    let policy_digest = super::verifier::compute_hash(&CODE_V1);
    let policy_input = [3, 0, 0, 0];
    let transport_snapshot = TransportSnapshot::new(Some(7), Some(1))
        .with_congestion_marks(Some(2))
        .with_retransmissions(Some(1));
    let event = RawEvent::new(10, ids::ROUTE_DECISION)
        .with_arg0(0)
        .with_arg1(99);
    let expected = execute_policy_verdict(
        &catalog,
        policy_digest,
        slot,
        mode,
        &event,
        policy_input,
        transport_snapshot,
    )
    .expect("expected verdict");

    push_policy_audit_tuple(
        ring,
        10,
        slot,
        mode,
        policy_digest,
        ids::ROUTE_DECISION,
        0,
        99,
        policy_input,
        transport_snapshot,
        expected,
    );

    let mismatched = AuditVerdict {
        verdict_meta: (2u32 << 24) | (1u32 << 16),
        reason: 0xFFFF,
        fuel_used: expected.fuel_used,
    };
    push_policy_audit_tuple(
        ring,
        11,
        slot,
        mode,
        policy_digest,
        ids::ROUTE_DECISION,
        0,
        99,
        policy_input,
        transport_snapshot,
        mismatched,
    );

    let result = replay_verdict_from_audit(ring, &mut cursor, &catalog);
    assert!(matches!(result, Err("verdict replay mismatch")));

    restore_ring(ring, previous);
}

#[test]
fn policy_replay_with_defer_matches() {
    let _guard = policy_replay_test_lock()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let (ring, previous) = install_test_ring();
    let mut cursor = ring.head();
    let catalog = replay_catalog();

    let slot = super::vm::Slot::Route;
    let mode = PolicyMode::Enforce;
    let policy_digest = super::verifier::compute_hash(&CODE_V1);
    let policy_input = [5, 0, 0, 0];
    let transport_snapshot = TransportSnapshot::new(Some(5), Some(1))
        .with_congestion_marks(Some(0))
        .with_retransmissions(Some(0));
    let event = RawEvent::new(20, ids::ROUTE_DECISION)
        .with_arg0(1)
        .with_arg1(7);
    let expected = execute_policy_verdict(
        &catalog,
        policy_digest,
        slot,
        mode,
        &event,
        policy_input,
        transport_snapshot,
    )
    .expect("expected verdict");
    push_policy_audit_tuple(
        ring,
        20,
        slot,
        mode,
        policy_digest,
        ids::ROUTE_DECISION,
        1,
        7,
        policy_input,
        transport_snapshot,
        expected,
    );
    ring.push(
        RawEvent::new(21, ids::POLICY_AUDIT_DEFER)
            .with_arg0((1u32 << 24) | (3u32 << 16) | 6u32)
            .with_arg1(1)
            .with_arg2((2u32 << 16) | 0),
    );

    let rows = replay_verdict_from_audit(ring, &mut cursor, &catalog).expect("replay must match");
    assert_eq!(rows, 1);

    restore_ring(ring, previous);
}
