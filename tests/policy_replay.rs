#![cfg(feature = "std")]

use hibana::substrate::{
    mgmt::tap::TapEvent,
    policy::epf::Slot,
    transport::{TransportAlgorithm, TransportSnapshot},
};

const POLICY_COMMIT_ID: u16 = 0x0405;
const POLICY_ROLLBACK_ID: u16 = 0x0406;
const POLICY_AUDIT_ID: u16 = 0x0407;
const POLICY_AUDIT_EXT_ID: u16 = 0x0408;
const POLICY_AUDIT_RESULT_ID: u16 = 0x0409;
const POLICY_REPLAY_EVENT_ID: u16 = 0x040A;
const POLICY_REPLAY_INPUT0_ID: u16 = 0x040B;
const POLICY_REPLAY_INPUT1_ID: u16 = 0x040C;
const POLICY_REPLAY_TRANSPORT0_ID: u16 = 0x040D;
const POLICY_REPLAY_TRANSPORT1_ID: u16 = 0x040E;
const POLICY_REPLAY_EVENT_EXT_ID: u16 = 0x040F;
const POLICY_AUDIT_DEFER_ID: u16 = 0x0410;
const ROUTE_DECISION_ID: u16 = 0x0221;
const TRANSPORT_EVENT_ID: u16 = 0x0212;
const REPLAY_LOG_CAPACITY: usize = 2048;
const AUDIT_ROW_CAPACITY: usize = 128;

fn raw_event(ts: u32, id: u16) -> TapEvent {
    TapEvent {
        ts,
        id,
        ..TapEvent::zero()
    }
}

struct ReplayLog {
    events: [TapEvent; REPLAY_LOG_CAPACITY],
    len: usize,
}

impl ReplayLog {
    fn push(&mut self, event: TapEvent) {
        assert!(
            self.len < REPLAY_LOG_CAPACITY,
            "replay log capacity exceeded"
        );
        self.events[self.len] = event;
        self.len += 1;
    }

    fn head(&self) -> usize {
        self.len
    }

    fn events_since<'a>(&'a self, cursor: &mut usize) -> impl Iterator<Item = TapEvent> + 'a {
        let start = *cursor;
        let end = self.len;
        *cursor = end;
        self.events[start..end].iter().copied()
    }
}

impl Default for ReplayLog {
    fn default() -> Self {
        Self {
            events: [TapEvent::zero(); REPLAY_LOG_CAPACITY],
            len: 0,
        }
    }
}

struct AuditRows {
    rows: [Option<AuditRow>; AUDIT_ROW_CAPACITY],
    len: usize,
}

impl AuditRows {
    fn new() -> Self {
        Self {
            rows: [None; AUDIT_ROW_CAPACITY],
            len: 0,
        }
    }

    fn push(&mut self, row: AuditRow) {
        assert!(self.len < AUDIT_ROW_CAPACITY, "audit row capacity exceeded");
        self.rows[self.len] = Some(row);
        self.len += 1;
    }

    fn len(&self) -> usize {
        self.len
    }

    fn get(&self, idx: usize) -> Option<&AuditRow> {
        if idx < self.len {
            self.rows[idx].as_ref()
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DigestState {
    active_digest: Option<u32>,
    standby_digest: Option<u32>,
    last_good_digest: Option<u32>,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct AuditRow {
    digest: u32,
    slot: Slot,
    mode_tag: u8,
    event: TapEvent,
    policy_input: [u32; 4],
    transport_snapshot: TransportSnapshot,
    verdict_meta: u32,
    reason: u32,
    fuel_used: u32,
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

fn slot_tag(slot: Slot) -> u8 {
    match slot {
        Slot::Forward => 0,
        Slot::EndpointRx => 1,
        Slot::EndpointTx => 2,
        Slot::Rendezvous => 3,
        Slot::Route => 4,
    }
}

fn replay_digests(log: &ReplayLog, slot: Slot, cursor: &mut usize) -> DigestState {
    let mut replay = DigestState {
        active_digest: None,
        standby_digest: None,
        last_good_digest: None,
    };
    for event in log.events_since(cursor) {
        if event.arg0 != slot_tag(slot) as u32 {
            continue;
        }
        match event.id {
            POLICY_COMMIT_ID => {
                if let Some(current) = replay.active_digest {
                    replay.last_good_digest = Some(current);
                }
                replay.active_digest = Some(event.arg2);
                replay.standby_digest = None;
            }
            POLICY_ROLLBACK_ID => {
                replay.standby_digest = replay.active_digest;
                replay.active_digest = Some(event.arg2);
                replay.last_good_digest = Some(event.arg2);
            }
            _ => {}
        }
    }
    replay
}

const FNV32_OFFSET: u32 = 0x811C_9DC5;
const FNV32_PRIME: u32 = 0x0100_0193;

fn fnv32_mix_u8(mut hash: u32, byte: u8) -> u32 {
    hash ^= byte as u32;
    hash.wrapping_mul(FNV32_PRIME)
}

fn fnv32_mix_u16(hash: u32, value: u16) -> u32 {
    let bytes = value.to_le_bytes();
    let hash = fnv32_mix_u8(hash, bytes[0]);
    fnv32_mix_u8(hash, bytes[1])
}

fn fnv32_mix_u32(hash: u32, value: u32) -> u32 {
    let bytes = value.to_le_bytes();
    let hash = fnv32_mix_u8(hash, bytes[0]);
    let hash = fnv32_mix_u8(hash, bytes[1]);
    let hash = fnv32_mix_u8(hash, bytes[2]);
    fnv32_mix_u8(hash, bytes[3])
}

fn fnv32_mix_u64(hash: u32, value: u64) -> u32 {
    let bytes = value.to_le_bytes();
    let mut out = hash;
    let mut idx = 0usize;
    while idx < bytes.len() {
        out = fnv32_mix_u8(out, bytes[idx]);
        idx += 1;
    }
    out
}

fn fnv32_mix_opt_u32(hash: u32, value: Option<u32>) -> u32 {
    match value {
        Some(v) => fnv32_mix_u32(fnv32_mix_u8(hash, 1), v),
        None => fnv32_mix_u8(hash, 0),
    }
}

fn fnv32_mix_opt_u64(hash: u32, value: Option<u64>) -> u32 {
    match value {
        Some(v) => fnv32_mix_u64(fnv32_mix_u8(hash, 1), v),
        None => fnv32_mix_u8(hash, 0),
    }
}

fn hash_tap_event(event: &TapEvent) -> u32 {
    let mut hash = FNV32_OFFSET;
    hash = fnv32_mix_u32(hash, event.ts);
    hash = fnv32_mix_u16(hash, event.id);
    hash = fnv32_mix_u16(hash, event.causal_key);
    hash = fnv32_mix_u32(hash, event.arg0);
    hash = fnv32_mix_u32(hash, event.arg1);
    fnv32_mix_u32(hash, event.arg2)
}

fn hash_policy_input(input: [u32; 4]) -> u32 {
    let mut hash = FNV32_OFFSET;
    let mut idx = 0usize;
    while idx < input.len() {
        hash = fnv32_mix_u32(hash, input[idx]);
        idx += 1;
    }
    hash
}

fn hash_transport_snapshot(snapshot: TransportSnapshot) -> u32 {
    let mut hash = FNV32_OFFSET;
    hash = fnv32_mix_opt_u64(hash, snapshot.latency_us);
    hash = fnv32_mix_opt_u32(hash, snapshot.queue_depth);
    hash = fnv32_mix_opt_u64(hash, snapshot.pacing_interval_us);
    hash = fnv32_mix_opt_u32(hash, snapshot.congestion_marks);
    hash = fnv32_mix_opt_u32(hash, snapshot.retransmissions);
    hash = fnv32_mix_opt_u32(hash, snapshot.pto_count);
    hash = fnv32_mix_opt_u64(hash, snapshot.srtt_us);
    hash = fnv32_mix_opt_u64(hash, snapshot.latest_ack_pn);
    hash = fnv32_mix_opt_u64(hash, snapshot.congestion_window);
    hash = fnv32_mix_opt_u64(hash, snapshot.in_flight_bytes);
    match snapshot.algorithm {
        Some(TransportAlgorithm::Cubic) => fnv32_mix_u8(hash, 1),
        Some(TransportAlgorithm::Reno) => fnv32_mix_u8(hash, 2),
        Some(TransportAlgorithm::Other(code)) => fnv32_mix_u8(fnv32_mix_u8(hash, 3), code),
        None => fnv32_mix_u8(hash, 0),
    }
}

fn replay_transport_inputs(snapshot: TransportSnapshot) -> [u32; 4] {
    let latency = snapshot
        .latency_us
        .map(|value| value.min(u32::MAX as u64) as u32)
        .unwrap_or(0);
    [
        latency,
        snapshot.queue_depth.unwrap_or(0),
        snapshot.congestion_marks.unwrap_or(0),
        snapshot.retransmissions.unwrap_or(0),
    ]
}

fn replay_transport_presence(snapshot: TransportSnapshot) -> u8 {
    let mut mask = 0u8;
    if snapshot.latency_us.is_some() {
        mask |= 1 << 0;
    }
    if snapshot.queue_depth.is_some() {
        mask |= 1 << 1;
    }
    if snapshot.congestion_marks.is_some() {
        mask |= 1 << 2;
    }
    if snapshot.retransmissions.is_some() {
        mask |= 1 << 3;
    }
    mask
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

fn decode_slot_mode(raw: u32) -> Result<(Slot, u8), &'static str> {
    let slot = match ((raw >> 24) & 0xFF) as u8 {
        0 => Slot::Forward,
        1 => Slot::EndpointRx,
        2 => Slot::EndpointTx,
        3 => Slot::Rendezvous,
        4 => Slot::Route,
        _ => return Err("invalid slot tag"),
    };
    let mode = ((raw >> 16) & 0xFF) as u8;
    if mode > 1 {
        return Err("invalid mode tag");
    }
    Ok((slot, mode))
}

fn push_policy_audit_tuple(
    log: &mut ReplayLog,
    ts: u32,
    digest: u32,
    slot: Slot,
    mode_tag: u8,
    event: TapEvent,
    policy_input: [u32; 4],
    transport_snapshot: TransportSnapshot,
    verdict_meta: u32,
    reason: u32,
    fuel_used: u32,
) {
    let replay_transport = replay_transport_inputs(transport_snapshot);
    let replay_transport_presence = replay_transport_presence(transport_snapshot);
    log.push(
        raw_event(ts, POLICY_AUDIT_ID)
            .with_arg0(digest)
            .with_arg1(hash_tap_event(&event))
            .with_arg2(hash_policy_input(policy_input)),
    );
    log.push(
        raw_event(ts, POLICY_AUDIT_EXT_ID)
            .with_arg0(0)
            .with_arg1(hash_transport_snapshot(transport_snapshot))
            .with_arg2(((slot_tag(slot) as u32) << 24) | ((mode_tag as u32) << 16)),
    );
    log.push(
        raw_event(ts, POLICY_REPLAY_EVENT_ID)
            .with_arg0(event.ts)
            .with_arg1(event.id as u32)
            .with_arg2(event.arg0),
    );
    log.push(
        raw_event(ts, POLICY_REPLAY_EVENT_EXT_ID)
            .with_arg0(event.arg1)
            .with_arg1(event.arg2)
            .with_arg2(event.causal_key as u32),
    );
    log.push(
        raw_event(ts, POLICY_REPLAY_INPUT0_ID)
            .with_arg0(policy_input[0])
            .with_arg1(policy_input[1])
            .with_arg2(policy_input[2]),
    );
    log.push(
        raw_event(ts, POLICY_REPLAY_INPUT1_ID)
            .with_arg0(policy_input[3])
            .with_arg1(0),
    );
    log.push(
        raw_event(ts, POLICY_REPLAY_TRANSPORT0_ID)
            .with_arg0(replay_transport[0])
            .with_arg1(replay_transport[1])
            .with_arg2(replay_transport[2]),
    );
    log.push(
        raw_event(ts, POLICY_REPLAY_TRANSPORT1_ID)
            .with_arg0(replay_transport[3])
            .with_arg1(replay_transport_presence as u32),
    );
    log.push(
        raw_event(ts, POLICY_AUDIT_RESULT_ID)
            .with_arg0(verdict_meta)
            .with_arg1(reason)
            .with_arg2(fuel_used),
    );
}

fn replay_audit_rows(log: &ReplayLog, cursor: &mut usize) -> Result<AuditRows, &'static str> {
    let mut pending = ReplayPending::default();
    let mut rows = AuditRows::new();

    for event in log.events_since(cursor) {
        match event.id {
            POLICY_AUDIT_ID => {
                if !pending.is_empty() {
                    return Err("incomplete audit tuple");
                }
                pending.core = Some((event.arg0, event.arg1, event.arg2));
            }
            POLICY_AUDIT_EXT_ID => {
                if pending.core.is_none() {
                    return Err("incomplete audit tuple");
                }
                pending.ext = Some((event.arg0, event.arg1, event.arg2));
            }
            POLICY_REPLAY_EVENT_ID => {
                if pending.core.is_none() || pending.ext.is_none() {
                    return Err("incomplete audit tuple");
                }
                let event_id = u16::try_from(event.arg1).map_err(|_| "invalid replay event id")?;
                pending.replay_event = Some((event.arg0, event_id, event.arg2));
            }
            POLICY_REPLAY_EVENT_EXT_ID => {
                if pending.core.is_none() || pending.ext.is_none() || pending.replay_event.is_none()
                {
                    return Err("incomplete audit tuple");
                }
                let causal_key =
                    u16::try_from(event.arg2).map_err(|_| "invalid replay causal key")?;
                pending.replay_event_ext = Some((event.arg0, event.arg1, causal_key));
            }
            POLICY_REPLAY_INPUT0_ID => {
                if pending.core.is_none() {
                    return Err("incomplete audit tuple");
                }
                pending.replay_input0 = Some((event.arg0, event.arg1, event.arg2));
            }
            POLICY_REPLAY_INPUT1_ID => {
                if pending.core.is_none() {
                    return Err("incomplete audit tuple");
                }
                pending.replay_input1 = Some(event.arg0);
            }
            POLICY_REPLAY_TRANSPORT0_ID => {
                if pending.core.is_none() {
                    return Err("incomplete audit tuple");
                }
                pending.replay_transport0 = Some((event.arg0, event.arg1, event.arg2));
            }
            POLICY_REPLAY_TRANSPORT1_ID => {
                if pending.core.is_none() {
                    return Err("incomplete audit tuple");
                }
                let presence =
                    u8::try_from(event.arg1).map_err(|_| "invalid transport presence")?;
                pending.replay_transport1 = Some((event.arg0, presence));
            }
            POLICY_AUDIT_DEFER_ID => {
                let source = ((event.arg0 >> 24) & 0xFF) as u8;
                if source > 2 {
                    return Err("invalid defer source");
                }
                let reason = ((event.arg2 >> 16) & 0xFFFF) as u16;
                if reason != 1 && reason != 2 {
                    return Err("invalid defer reason");
                }
            }
            POLICY_AUDIT_RESULT_ID => {
                let Some((digest, event_hash, input_hash)) = pending.core.take() else {
                    return Err("incomplete audit tuple");
                };
                let Some((_, transport_hash, slot_mode)) = pending.ext.take() else {
                    return Err("incomplete audit tuple");
                };
                let Some((event_ts, event_id, event_arg0)) = pending.replay_event.take() else {
                    return Err("incomplete audit tuple");
                };
                let Some((event_arg1, event_arg2, event_causal_key)) =
                    pending.replay_event_ext.take()
                else {
                    return Err("incomplete audit tuple");
                };
                let Some((input0, input1, input2)) = pending.replay_input0.take() else {
                    return Err("incomplete audit tuple");
                };
                let Some(input3) = pending.replay_input1.take() else {
                    return Err("incomplete audit tuple");
                };
                let Some((latency, queue, congestion)) = pending.replay_transport0.take() else {
                    return Err("incomplete audit tuple");
                };
                let Some((retry, transport_presence)) = pending.replay_transport1.take() else {
                    return Err("incomplete audit tuple");
                };
                let (slot, mode_tag) = decode_slot_mode(slot_mode)?;

                let replay_event = raw_event(event_ts, event_id)
                    .with_causal_key(event_causal_key)
                    .with_arg0(event_arg0)
                    .with_arg1(event_arg1)
                    .with_arg2(event_arg2);
                if hash_tap_event(&replay_event) != event_hash {
                    return Err("event hash mismatch");
                }

                let policy_input = [input0, input1, input2, input3];
                if hash_policy_input(policy_input) != input_hash {
                    return Err("input hash mismatch");
                }

                let transport_snapshot = replay_transport_snapshot(
                    [latency, queue, congestion, retry],
                    transport_presence,
                );
                if hash_transport_snapshot(transport_snapshot) != transport_hash {
                    return Err("transport hash mismatch");
                }

                rows.push(AuditRow {
                    digest,
                    slot,
                    mode_tag,
                    event: replay_event,
                    policy_input,
                    transport_snapshot,
                    verdict_meta: event.arg0,
                    reason: event.arg1,
                    fuel_used: event.arg2,
                });
                pending.clear();
            }
            _ => {}
        }
    }

    if !pending.is_empty() {
        return Err("incomplete audit tuple");
    }
    Ok(rows)
}

#[test]
fn replay_from_audit_log_tracks_digest_transitions() {
    let mut log = ReplayLog::default();
    let mut cursor = log.head();
    let digest_v1 = 0x1020_3040;
    let digest_v2 = 0x5060_7080;

    log.push(
        raw_event(1, POLICY_COMMIT_ID)
            .with_arg0(slot_tag(Slot::Route) as u32)
            .with_arg1(1)
            .with_arg2(digest_v1),
    );
    log.push(
        raw_event(2, POLICY_COMMIT_ID)
            .with_arg0(slot_tag(Slot::Route) as u32)
            .with_arg1(2)
            .with_arg2(digest_v2),
    );
    log.push(
        raw_event(3, POLICY_ROLLBACK_ID)
            .with_arg0(slot_tag(Slot::Route) as u32)
            .with_arg1(1)
            .with_arg2(digest_v1),
    );

    let replay = replay_digests(&log, Slot::Route, &mut cursor);
    assert_eq!(
        replay,
        DigestState {
            active_digest: Some(digest_v1),
            standby_digest: Some(digest_v2),
            last_good_digest: Some(digest_v1),
        }
    );
}

#[test]
fn public_policy_audit_tuple_roundtrips_logged_inputs() {
    let mut log = ReplayLog::default();
    let mut cursor = log.head();

    let event_one = raw_event(11, ROUTE_DECISION_ID)
        .with_causal_key(7)
        .with_arg0(1)
        .with_arg1(42)
        .with_arg2(99);
    let input_one = [9, 8, 7, 6];
    let transport_one = TransportSnapshot::new(Some(15), Some(3))
        .with_congestion_marks(Some(2))
        .with_retransmissions(Some(1));
    push_policy_audit_tuple(
        &mut log,
        100,
        0xAABB_CCDD,
        Slot::Route,
        1,
        event_one,
        input_one,
        transport_one,
        0x0102_0000,
        0,
        5,
    );

    let event_two = raw_event(12, TRANSPORT_EVENT_ID).with_arg0(3).with_arg1(4);
    let input_two = [1, 2, 3, 4];
    let transport_two = TransportSnapshot::new(None, Some(1))
        .with_congestion_marks(None)
        .with_retransmissions(Some(0));
    push_policy_audit_tuple(
        &mut log,
        101,
        0x1122_3344,
        Slot::EndpointRx,
        0,
        event_two,
        input_two,
        transport_two,
        0,
        7,
        9,
    );
    log.push(
        raw_event(102, POLICY_AUDIT_DEFER_ID)
            .with_arg0((2u32 << 24) | (1u32 << 16) | 4u32)
            .with_arg1(0)
            .with_arg2((1u32 << 16) | 0),
    );

    let rows = replay_audit_rows(&log, &mut cursor).expect("audit tuples must roundtrip");
    assert_eq!(rows.len(), 2);

    let first = rows.get(0).expect("first audit row");
    assert_eq!(first.digest, 0xAABB_CCDD);
    assert_eq!(first.slot, Slot::Route);
    assert_eq!(first.mode_tag, 1);
    assert_eq!(first.event, event_one);
    assert_eq!(first.policy_input, input_one);
    assert_eq!(first.transport_snapshot, transport_one);
    assert_eq!(first.verdict_meta, 0x0102_0000);
    assert_eq!(first.fuel_used, 5);

    let second = rows.get(1).expect("second audit row");
    assert_eq!(second.digest, 0x1122_3344);
    assert_eq!(second.slot, Slot::EndpointRx);
    assert_eq!(second.mode_tag, 0);
    assert_eq!(second.event, event_two);
    assert_eq!(second.policy_input, input_two);
    assert_eq!(second.transport_snapshot, transport_two);
    assert_eq!(second.reason, 7);
}

#[test]
fn public_policy_audit_tuple_rejects_corruption() {
    let mut log = ReplayLog::default();
    let mut cursor = log.head();

    let event = raw_event(21, ROUTE_DECISION_ID).with_arg0(1).with_arg1(2);
    let input = [4, 3, 2, 1];
    let transport = TransportSnapshot::new(Some(9), Some(1))
        .with_congestion_marks(Some(0))
        .with_retransmissions(Some(0));
    push_policy_audit_tuple(
        &mut log,
        200,
        0xDEAD_BEEF,
        Slot::Route,
        1,
        event,
        input,
        transport,
        0,
        0,
        1,
    );
    log.push(
        raw_event(201, POLICY_AUDIT_DEFER_ID)
            .with_arg0((1u32 << 24) | (3u32 << 16) | 6u32)
            .with_arg1(1)
            .with_arg2((9u32 << 16) | 0),
    );

    let result = replay_audit_rows(&log, &mut cursor);
    assert!(matches!(result, Err("invalid defer reason")));
}
