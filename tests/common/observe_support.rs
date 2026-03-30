use hibana::substrate::mgmt::tap::TapEvent;

pub(crate) const LANE_ACQUIRE_ID: u16 = 0x0210;
pub(crate) const LANE_RELEASE_ID: u16 = 0x0211;
pub(crate) const LOCAL_ACTION_FAIL_ID: u16 = 0x0226;
pub(crate) const POLICY_ABORT_ID: u16 = 0x0400;
pub(crate) const POLICY_ANNOT_ID: u16 = 0x0401;
pub(crate) const POLICY_TRAP_ID: u16 = 0x0402;
pub(crate) const POLICY_EFFECT_ID: u16 = 0x0403;
pub(crate) const POLICY_RA_OK_ID: u16 = 0x0404;
pub(crate) const POLICY_COMMIT_ID: u16 = 0x0405;
pub(crate) const POLICY_ROLLBACK_ID: u16 = 0x0406;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PolicyEventKind {
    Abort,
    Trap,
    Annotate,
    Effect,
    EffectOk,
    Commit,
    Rollback,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PolicyEventDomain {
    Policy,
    Epf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PolicyLaneRecord {
    pub(crate) kind: PolicyEventKind,
    pub(crate) policy_id: u16,
    pub(crate) domain: PolicyEventDomain,
    pub(crate) lane: Option<u16>,
    pub(crate) has_association: bool,
    pub(crate) sid_hint: Option<u32>,
    pub(crate) sid_match: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LaneAssociation {
    sid: u32,
}

fn ensure_lane_capacity(table: &mut Vec<Option<LaneAssociation>>, lane: usize) {
    if lane >= table.len() {
        table.resize(lane + 1, None);
    }
}

fn decode_lane_event(event: TapEvent) -> Option<(u16, Option<LaneAssociation>)> {
    let sid = event.arg1 >> 16;
    let lane = (event.arg1 & 0xFFFF) as u16;
    match event.id {
        id if id == LANE_ACQUIRE_ID => Some((lane, Some(LaneAssociation { sid }))),
        id if id == LANE_RELEASE_ID => Some((lane, None)),
        _ => None,
    }
}

fn policy_event_lane(event: TapEvent) -> Option<u16> {
    match event.causal_role() {
        0 => None,
        lane => Some(u16::from(lane.saturating_sub(1))),
    }
}

fn policy_event_meta(
    event: TapEvent,
) -> Option<(u16, PolicyEventKind, PolicyEventDomain, Option<u32>)> {
    let sid_hint = if event.arg1 != 0 {
        Some(event.arg1)
    } else {
        None
    };
    match event.id {
        id if id == POLICY_ABORT_ID => Some((
            id,
            PolicyEventKind::Abort,
            PolicyEventDomain::Policy,
            sid_hint,
        )),
        id if id == POLICY_TRAP_ID => Some((
            id,
            PolicyEventKind::Trap,
            PolicyEventDomain::Policy,
            sid_hint,
        )),
        id if id == POLICY_ANNOT_ID => {
            Some((id, PolicyEventKind::Annotate, PolicyEventDomain::Epf, None))
        }
        id if id == POLICY_EFFECT_ID => {
            Some((id, PolicyEventKind::Effect, PolicyEventDomain::Epf, None))
        }
        id if id == POLICY_RA_OK_ID => Some((
            id,
            PolicyEventKind::EffectOk,
            PolicyEventDomain::Epf,
            sid_hint,
        )),
        id if id == POLICY_COMMIT_ID => {
            Some((id, PolicyEventKind::Commit, PolicyEventDomain::Policy, None))
        }
        id if id == POLICY_ROLLBACK_ID => Some((
            id,
            PolicyEventKind::Rollback,
            PolicyEventDomain::Policy,
            None,
        )),
        _ => None,
    }
}

pub(crate) fn policy_lane_trace(
    storage: &[TapEvent],
    start: usize,
    end: usize,
) -> (Vec<PolicyLaneRecord>, usize) {
    let mut records = Vec::new();
    let mut local_action_failures = 0usize;
    let mut lane_table: Vec<Option<LaneAssociation>> = Vec::new();
    let capacity = storage.len();
    let mut cursor = start;

    while cursor < end {
        let raw = storage[cursor % capacity];

        if let Some((lane, assoc)) = decode_lane_event(raw) {
            let lane_idx = lane as usize;
            ensure_lane_capacity(&mut lane_table, lane_idx);
            lane_table[lane_idx] = assoc;
        }

        if raw.id == LOCAL_ACTION_FAIL_ID {
            local_action_failures += 1;
        }

        if let Some((policy_id, kind, domain, sid_hint)) = policy_event_meta(raw) {
            let lane = policy_event_lane(raw);
            let association =
                lane.and_then(|lane_id| lane_table.get(lane_id as usize).copied().flatten());
            let sid_match = association.and_then(|assoc| sid_hint.map(|sid| sid == assoc.sid));
            records.push(PolicyLaneRecord {
                kind,
                policy_id,
                domain,
                lane,
                has_association: association.is_some(),
                sid_hint,
                sid_match,
            });
        }

        cursor += 1;
    }

    (records, local_action_failures)
}
