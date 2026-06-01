#![cfg(feature = "std")]

use hibana::integration::runtime::TapEvent;

const POLICY_COMMIT_ID: u16 = 0x0405;
const POLICY_STATE_RESTORE_ID: u16 = 0x0406;
const POLICY_TX_ABORT_ID: u16 = 0x0411;
const POLICY_AUDIT_ID: u16 = 0x0407;
const POLICY_AUDIT_EXT_ID: u16 = 0x0408;
const POLICY_AUDIT_RESULT_ID: u16 = 0x0409;
const POLICY_REPLAY_EVENT_ID: u16 = 0x040A;
const POLICY_REPLAY_INPUT0_ID: u16 = 0x040B;
const POLICY_REPLAY_INPUT1_ID: u16 = 0x040C;
const POLICY_REPLAY_ATTRS0_ID: u16 = 0x040D;
const POLICY_REPLAY_ATTRS1_ID: u16 = 0x040E;
const POLICY_REPLAY_EVENT_EXT_ID: u16 = 0x040F;
const POLICY_AUDIT_DEFER_ID: u16 = 0x0410;
const ROUTE_DECISION_ID: u16 = 0x0221;
const ENDPOINT_RX_EVENT_ID: u16 = 0x0212;
const REPLAY_LOG_CAPACITY: usize = 2048;
const AUDIT_ROW_CAPACITY: usize = 128;
const DEFER_SOURCE_RESOLVER: u8 = 0x80;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PolicySlot {
    Forward,
    EndpointRx,
    EndpointTx,
    Rendezvous,
    Decision,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ReplayAttrs {
    latency_us: Option<u64>,
    queue_depth: Option<u32>,
}

fn policy_attrs(latency_us: Option<u64>, queue_depth: Option<u32>) -> ReplayAttrs {
    ReplayAttrs {
        latency_us,
        queue_depth,
    }
}

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
    replay_attrs0: Option<(u32, u32, u32)>,
    replay_attrs1: Option<(u32, u8)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct AuditRow {
    digest: u32,
    slot: PolicySlot,
    mode_tag: u8,
    event: TapEvent,
    policy_input: [u32; 4],
    policy_attrs: ReplayAttrs,
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
            && self.replay_attrs0.is_none()
            && self.replay_attrs1.is_none()
    }

    #[inline]
    fn clear(&mut self) {
        *self = Self::default();
    }
}

fn slot_tag(slot: PolicySlot) -> u8 {
    match slot {
        PolicySlot::Forward => 0,
        PolicySlot::EndpointRx => 1,
        PolicySlot::EndpointTx => 2,
        PolicySlot::Rendezvous => 3,
        PolicySlot::Decision => 4,
    }
}

fn replay_digests(log: &ReplayLog, slot: PolicySlot, cursor: &mut usize) -> DigestState {
    let mut replay = DigestState {
        active_digest: None,
        standby_digest: None,
        last_good_digest: None,
    };
    let rows = replay_audit_rows(log, cursor).expect("digest replay requires complete audit rows");
    let mut idx = 0usize;
    while idx < rows.len() {
        let row = rows.get(idx).expect("audit row");
        idx += 1;
        if row.slot != slot {
            continue;
        }
        match row.event.id {
            POLICY_COMMIT_ID => {
                if let Some(current) = replay.active_digest {
                    replay.last_good_digest = Some(current);
                }
                replay.active_digest = Some(row.digest);
                replay.standby_digest = None;
            }
            POLICY_STATE_RESTORE_ID => {
                replay.standby_digest = replay.active_digest;
                replay.active_digest = Some(row.digest);
                replay.last_good_digest = Some(row.digest);
            }
            POLICY_TX_ABORT_ID => {
                replay.standby_digest = replay.active_digest;
                replay.active_digest = Some(row.digest);
                replay.last_good_digest = Some(row.digest);
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

fn hash_policy_attrs(attrs: &ReplayAttrs) -> u32 {
    let mut hash = FNV32_OFFSET;
    hash = fnv32_mix_opt_u64(hash, attrs.latency_us);
    fnv32_mix_opt_u32(hash, attrs.queue_depth)
}

fn replay_policy_attr_words(attrs: &ReplayAttrs) -> [u32; 4] {
    let latency = attrs
        .latency_us
        .map(|value| value.min(u32::MAX as u64) as u32)
        .unwrap_or(0);
    [latency, attrs.queue_depth.unwrap_or(0), 0, 0]
}

fn replay_policy_attr_presence(attrs: &ReplayAttrs) -> u8 {
    let mut mask = 0u8;
    if attrs.latency_us.is_some() {
        mask |= 1 << 0;
    }
    if attrs.queue_depth.is_some() {
        mask |= 1 << 1;
    }
    mask
}

fn replay_policy_attrs(values: [u32; 4], presence: u8) -> ReplayAttrs {
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
    policy_attrs(latency, queue_depth)
}

fn decode_slot_mode(raw: u32) -> Result<(PolicySlot, u8), &'static str> {
    let slot = match ((raw >> 24) & 0xFF) as u8 {
        0 => PolicySlot::Forward,
        1 => PolicySlot::EndpointRx,
        2 => PolicySlot::EndpointTx,
        3 => PolicySlot::Rendezvous,
        4 => PolicySlot::Decision,
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
    slot: PolicySlot,
    mode_tag: u8,
    event: TapEvent,
    policy_input: [u32; 4],
    policy_attrs: ReplayAttrs,
    verdict_meta: u32,
    reason: u32,
    fuel_used: u32,
) {
    let replay_attrs = replay_policy_attr_words(&policy_attrs);
    let replay_policy_attr_presence = replay_policy_attr_presence(&policy_attrs);
    log.push(
        raw_event(ts, POLICY_AUDIT_ID)
            .with_arg0(digest)
            .with_arg1(hash_tap_event(&event))
            .with_arg2(hash_policy_input(policy_input)),
    );
    log.push(
        raw_event(ts, POLICY_AUDIT_EXT_ID)
            .with_arg0(0)
            .with_arg1(hash_policy_attrs(&policy_attrs))
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
        raw_event(ts, POLICY_REPLAY_ATTRS0_ID)
            .with_arg0(replay_attrs[0])
            .with_arg1(replay_attrs[1])
            .with_arg2(replay_attrs[2]),
    );
    log.push(
        raw_event(ts, POLICY_REPLAY_ATTRS1_ID)
            .with_arg0(replay_attrs[3])
            .with_arg1(replay_policy_attr_presence as u32),
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
            POLICY_REPLAY_ATTRS0_ID => {
                if pending.core.is_none() {
                    return Err("incomplete audit tuple");
                }
                pending.replay_attrs0 = Some((event.arg0, event.arg1, event.arg2));
            }
            POLICY_REPLAY_ATTRS1_ID => {
                if pending.core.is_none() {
                    return Err("incomplete audit tuple");
                }
                let presence =
                    u8::try_from(event.arg1).map_err(|_| "invalid policy attr presence")?;
                pending.replay_attrs1 = Some((event.arg0, presence));
            }
            POLICY_AUDIT_DEFER_ID => {
                let source = ((event.arg0 >> 24) & 0xFF) as u8;
                if source != DEFER_SOURCE_RESOLVER {
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
                let Some((_, policy_attrs_hash, slot_mode)) = pending.ext.take() else {
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
                let Some((latency, queue, reserved0)) = pending.replay_attrs0.take() else {
                    return Err("incomplete audit tuple");
                };
                let Some((reserved1, policy_attr_presence)) = pending.replay_attrs1.take() else {
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

                let policy_attrs = replay_policy_attrs(
                    [latency, queue, reserved0, reserved1],
                    policy_attr_presence,
                );
                if hash_policy_attrs(&policy_attrs) != policy_attrs_hash {
                    return Err("policy attr hash mismatch");
                }

                rows.push(AuditRow {
                    digest,
                    slot,
                    mode_tag,
                    event: replay_event,
                    policy_input,
                    policy_attrs,
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

#[path = "policy_replay/scenarios.rs"]
mod scenarios;
