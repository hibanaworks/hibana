//! Normalisation helpers for tap traces. Enabled by `cfg(test)` or the
//! `std` cargo feature so host tooling can inspect relay/endpoint events
//! without reimplementing the decoding logic.
use crate::control::cap::{EndpointResource, ResourceKind};
use crate::global::typestate::ScopeRegion;
use crate::observe::local::LocalActionFailure;
use crate::observe::scope::{ScopeTrace, tap_scope};
use crate::observe::{
    CancelEvent, CancelEventKind, PolicyEvent, PolicyEventKind, PolicyEventSpec,
    PolicyLaneExpectation, TapEvent, ids, policy_event_spec,
};
use crate::runtime::{consts::{RING_BUFFER_SIZE, RING_EVENTS}, mgmt::MgmtFacetProfile};
use crate::transport::TransportAlgorithm;

use std::{collections::BTreeMap, vec::Vec};

/// High-level view of forwarder events as seen in the tap trace.
///
/// This enum provides a normalized representation of forwarding operations,
/// abstracting over implementation details (relay vs splice).
///
/// # Observational equivalence
///
/// Despite different underlying mechanisms:
/// - `Relay`: Frame-by-frame copying (generates one `RELAY_FORWARD` tap event per frame)
/// - `Splice`: Two-step direct connection (generates `SPLICE_BEGIN`+`SPLICE_COMMIT` pairs)
///
/// Both collapse to the same `ForwardEvent` sequence when processed by
/// [`forward_trace()`]. This demonstrates the fundamental MPST invariant:
/// **relay ≡ splice** (observational equivalence / cut elimination).
///
/// # Example
///
/// ```rust,ignore
/// // Relay generates: RELAY_FORWARD event
/// forward.relay(frame).await?;
/// // → ForwardEvent::Relay { sid, lane, role, label, flags }
///
/// // Splice generates: SPLICE_BEGIN + SPLICE_COMMIT events
/// forward.splice(&src, gen).await?;
/// // → ForwardEvent::Splice { sid, generation }
/// ```
///
/// Use [`equivalence_key()`](ForwardEvent::equivalence_key) to compare traces
/// for observational equivalence.
///
/// See `examples/relay_splice_equivalence.rs` for a complete demonstration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForwardEvent {
    Relay {
        sid: u32,
        lane: u8,
        role: u8,
        label: u8,
        flags: u8,
    },
    Splice {
        sid: u32,
        generation: u32,
    },
}

impl ForwardEvent {
    #[inline]
    pub fn sid(&self) -> u32 {
        match *self {
            ForwardEvent::Relay { sid, .. } | ForwardEvent::Splice { sid, .. } => sid,
        }
    }

    /// Extract the equivalence key for observational comparison.
    ///
    /// Two `ForwardEvent`s are considered observationally equivalent if they
    /// have the same `equivalence_key()`. This is used to verify that relay
    /// and splice produce equivalent traces.
    ///
    /// Currently, equivalence is based solely on session ID, abstracting over
    /// implementation-specific details (lane/role/label for relay, generation for splice).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let relay_trace = normalise::forward_trace(relay_tap, start, end);
    /// let splice_trace = normalise::forward_trace(splice_tap, start, end);
    ///
    /// for (relay, splice) in relay_trace.iter().zip(splice_trace.iter()) {
    ///     assert_eq!(relay.equivalence_key(), splice.equivalence_key());
    /// }
    /// ```
    #[inline]
    pub fn equivalence_key(&self) -> ForwardEquivalenceKey {
        ForwardEquivalenceKey { sid: self.sid() }
    }

    #[inline]
    pub fn sort_key(&self) -> (u8, u32, u8, u8, u8, u32) {
        match *self {
            ForwardEvent::Relay {
                sid,
                lane,
                role,
                label,
                flags,
            } => (0, sid, lane, role, label, flags as u32),
            ForwardEvent::Splice { sid, generation } => (1, sid, 0, 0, 0, generation),
        }
    }
}

/// Boundary events emitted by `Endpoint::reroute`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DelegationEvent {
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
    Abort {
        sid: u32,
        lane: u8,
        reason: u8,
    },
    SloBreach {
        sid: u32,
        latency_us: u32,
        queue_tail: u16,
        retry: bool,
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

/// Normalised SLO breach event emitted by policy bytecode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SloBreachEvent {
    pub sid: u32,
    pub latency_us: u32,
    pub queue_tail: u16,
    pub retry: bool,
}

impl DelegationEvent {
    #[inline]
    pub fn sid(&self) -> u32 {
        match *self {
            DelegationEvent::Begin { sid, .. }
            | DelegationEvent::Pick { sid, .. }
            | DelegationEvent::Splice { sid, .. }
            | DelegationEvent::Abort { sid, .. }
            | DelegationEvent::SloBreach { sid, .. }
            | DelegationEvent::RouteDecision { sid, .. } => sid,
        }
    }

    #[inline]
    pub fn sort_key(&self) -> (u8, u32) {
        match *self {
            DelegationEvent::Begin { sid, .. } => (0, sid),
            DelegationEvent::Pick { sid, .. } => (1, sid),
            DelegationEvent::Splice { sid, .. } => (2, sid),
            DelegationEvent::SloBreach { sid, .. } => (3, sid),
            DelegationEvent::Abort { sid, .. } => (4, sid),
            DelegationEvent::RouteDecision { sid, .. } => (5, sid),
        }
    }
}

/// Lane ownership transitions as observed through tap events.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaneEvent {
    Acquire { rv: u32, sid: u32, lane: u16 },
    Release { rv: u32, sid: u32, lane: u16 },
}

impl LaneEvent {
    #[inline]
    fn decode_sid_lane(packed: u32) -> (u32, u16) {
        let sid = packed >> 16;
        let lane = (packed & 0xFFFF) as u16;
        (sid, lane)
    }

    #[inline]
    pub fn from_tap(event: TapEvent) -> Option<Self> {
        let (sid, lane) = Self::decode_sid_lane(event.arg1);
        match event.id {
            id if id == ids::LANE_ACQUIRE => Some(LaneEvent::Acquire {
                rv: event.arg0,
                sid,
                lane,
            }),
            id if id == ids::LANE_RELEASE => Some(LaneEvent::Release {
                rv: event.arg0,
                sid,
                lane,
            }),
            _ => None,
        }
    }
}

/// Extract lane events between the provided indices (monotonic head positions).
pub fn lane_trace(events: &[TapEvent], start: usize, end: usize) -> Vec<LaneEvent> {
    let mut trace = Vec::new();
    let mut index = start;
    while index < end {
        let event = events[index % RING_EVENTS];
        if let Some(mapped) = LaneEvent::from_tap(event) {
            trace.push(mapped);
        }
        index += 1;
    }
    trace
}

/// Association observed for a lane at a given point in the trace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LaneAssociation {
    pub rv: u32,
    pub sid: u32,
}

/// Policy tap event enriched with the current lane association (when available).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PolicyLaneRecord {
    pub spec: PolicyEventSpec,
    pub event: PolicyEvent,
    pub lane: Option<u16>,
    pub association: Option<LaneAssociation>,
    pub sid_hint: Option<u32>,
    pub sid_match: Option<bool>,
    pub scope: Option<ScopeTrace>,
}

impl PolicyLaneRecord {
    /// Returns `true` when the policy event could be matched to an active lane.
    #[inline]
    pub fn lane_matches(&self) -> bool {
        match self.spec.lane_expectation() {
            PolicyLaneExpectation::None => true,
            PolicyLaneExpectation::Optional => self.lane.is_none() || self.association.is_some(),
            PolicyLaneExpectation::Required => self.association.is_some(),
        }
    }

    /// Human-readable label for the underlying policy event kind.
    #[inline]
    pub fn name(&self) -> &'static str {
        self.spec.name()
    }
}

fn ensure_lane_capacity(table: &mut Vec<Option<LaneAssociation>>, lane: usize) {
    if lane >= table.len() {
        table.resize(lane + 1, None);
    }
}

/// Align policy events with the lane lifecycle observed in the tap trace.
///
/// This helper walks the tap range `[start, end)` once, maintaining a shadow view
/// of lane ownership inferred from `LANE_ACQUIRE`/`LANE_RELEASE` events. Each
/// policy event is then annotated with the lane association active at the moment
/// it was observed.
pub fn policy_lane_trace(
    storage: &[TapEvent],
    start: usize,
    end: usize,
) -> (Vec<PolicyLaneRecord>, Vec<LocalActionFailure>) {
    let capacity = storage.len();
    debug_assert!(capacity == RING_EVENTS || capacity == RING_BUFFER_SIZE);
    let mut records = Vec::new();
    let mut failures = Vec::new();
    let mut cursor = start;
    let mut lane_table: Vec<Option<LaneAssociation>> = Vec::new();

    while cursor < end {
        let raw = storage[cursor % capacity];

        if let Some(lane_event) = LaneEvent::from_tap(raw) {
            match lane_event {
                LaneEvent::Acquire { rv, sid, lane } => {
                    let lane_idx = lane as usize;
                    ensure_lane_capacity(&mut lane_table, lane_idx);
                    lane_table[lane_idx] = Some(LaneAssociation { rv, sid });
                }
                LaneEvent::Release { lane, .. } => {
                    let lane_idx = lane as usize;
                    if lane_idx < lane_table.len() {
                        lane_table[lane_idx] = None;
                    }
                }
            }
        }

        if let Some(spec) = policy_event_spec(raw.id)
            && let Some(event) = PolicyEvent::from_tap(raw)
        {
            let lane = event.lane.map(|lane| lane as u16);
            let association = lane.and_then(|lane_id| {
                let lane_idx = lane_id as usize;
                if lane_idx < lane_table.len() {
                    lane_table[lane_idx]
                } else {
                    None
                }
            });
            let sid_hint = spec.sid_hint(&event);
            let sid_match = association.and_then(|assoc| sid_hint.map(|sid| sid == assoc.sid));
            records.push(PolicyLaneRecord {
                spec,
                event,
                lane,
                association,
                sid_hint,
                sid_match,
                scope: tap_scope(&raw),
            });
        }

        if let Some(failure) = LocalActionFailure::from_tap(raw) {
            failures.push(failure);
        }

        cursor += 1;
    }

    (records, failures)
}

/// Extract management policy events (commit/rollback) with lane annotations.
pub fn mgmt_policy_trace(storage: &[TapEvent], start: usize, end: usize) -> Vec<PolicyLaneRecord> {
    policy_lane_trace(storage, start, end)
        .0
        .into_iter()
        .filter(|record| MgmtFacetProfile::supports_policy_kind(record.spec.kind))
        .collect()
}

/// Aggregated view of management policy events extracted from the tap trace.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MgmtPolicySummary {
    /// Number of management `POLICY_COMMIT` events observed.
    pub commits: usize,
    /// Number of management `POLICY_ROLLBACK` events observed.
    pub rollbacks: usize,
    /// Number of events whose lane association did not satisfy the expectation.
    pub unmatched_lanes: usize,
    /// Number of events whose session identifier did not match the recorded lane association.
    pub sid_mismatches: usize,
}

impl MgmtPolicySummary {
    /// Total count of management policy events considered in this summary.
    #[inline]
    pub const fn total(&self) -> usize {
        self.commits + self.rollbacks
    }

    /// Returns `true` when either lane matching or session identifier matching failed.
    #[inline]
    pub const fn has_alerts(&self) -> bool {
        self.unmatched_lanes > 0 || self.sid_mismatches > 0
    }
}

/// Build an aggregated summary for the provided management policy events.
pub fn mgmt_policy_summary(records: &[PolicyLaneRecord]) -> MgmtPolicySummary {
    let mut summary = MgmtPolicySummary::default();
    for record in records {
        match record.spec.kind {
            PolicyEventKind::Commit => summary.commits += 1,
            PolicyEventKind::Rollback => summary.rollbacks += 1,
            _ => {}
        }

        if !record.lane_matches() {
            summary.unmatched_lanes += 1;
        }

        if matches!(record.sid_match, Some(false)) {
            summary.sid_mismatches += 1;
        }
    }
    summary
}

/// Aggregated traces grouped by a shared [`ScopeTrace`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScopeCorrelatedTraces {
    /// Endpoint events tagged with this scope.
    pub endpoint: Vec<EndpointEvent>,
    /// Policy tap events tagged with this scope.
    pub policy: Vec<PolicyLaneRecord>,
    /// Management policy events tagged with this scope.
    pub mgmt: Vec<PolicyLaneRecord>,
}

/// Correlated traces annotated with [`ScopeRegion`] metadata when available.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScopeAnnotatedCorrelatedTraces {
    pub region: Option<ScopeRegion>,
    pub traces: ScopeCorrelatedTraces,
}

/// Build a correlation map keyed by [`ScopeTrace`] so higher-level tooling can
/// stitch together endpoint, policy, and management perspectives for the same
/// structured scope.
pub fn correlate_scope_traces(
    endpoint_events: &[EndpointEvent],
    policy_events: &[PolicyLaneRecord],
    mgmt_events: &[PolicyLaneRecord],
) -> BTreeMap<ScopeTrace, ScopeCorrelatedTraces> {
    let mut buckets = BTreeMap::new();

    for event in endpoint_events {
        if let Some(scope) = event.scope() {
            buckets
                .entry(scope)
                .or_insert_with(ScopeCorrelatedTraces::default)
                .endpoint
                .push(*event);
        }
    }

    for record in policy_events {
        if let Some(scope) = record.scope {
            buckets
                .entry(scope)
                .or_insert_with(ScopeCorrelatedTraces::default)
                .policy
                .push(record.clone());
        }
    }

    for record in mgmt_events {
        if let Some(scope) = record.scope {
            buckets
                .entry(scope)
                .or_insert_with(ScopeCorrelatedTraces::default)
                .mgmt
                .push(record.clone());
        }
    }

    buckets
}

/// Correlate scope traces and annotate them with [`ScopeRegion`] metadata.
pub fn correlate_scope_traces_with_atlas(
    endpoint_events: &[EndpointEvent],
    policy_events: &[PolicyLaneRecord],
    mgmt_events: &[PolicyLaneRecord],
    atlas: &[ScopeRegion],
) -> BTreeMap<ScopeTrace, ScopeAnnotatedCorrelatedTraces> {
    let base = correlate_scope_traces(endpoint_events, policy_events, mgmt_events);
    let mut annotated = BTreeMap::new();
    for (trace, traces) in base {
        let region = lookup_scope_region(atlas, trace);
        annotated.insert(trace, ScopeAnnotatedCorrelatedTraces { region, traces });
    }
    annotated
}

fn lookup_scope_region(atlas: &[ScopeRegion], trace: ScopeTrace) -> Option<ScopeRegion> {
    atlas
        .iter()
        .copied()
        .find(|region| region.range == trace.range && region.nest == trace.nest)
}

/// Events emitted by endpoints while they exchange frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndpointEvent {
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportTapEventKind {
    Ack,
    Loss,
    KeepaliveTx,
    KeepaliveRx,
    CloseStart,
    CloseDraining,
    CloseRemote,
}

/// Normalised representation of a transport telemetry tap event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransportTapEvent {
    pub kind: TransportTapEventKind,
    pub packet_number: u64,
    pub payload_len: u32,
    pub retransmissions: u32,
    pub pn_space: u8,
    pub cid_tag: u8,
}

impl TransportTapEvent {
    pub fn from_tap(event: TapEvent) -> Option<Self> {
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransportMetricsTapEvent {
    pub algorithm: TransportAlgorithm,
    pub queue_depth: Option<u32>,
    pub srtt_us: Option<u64>,
    pub congestion_window: Option<u64>,
    pub in_flight_bytes: Option<u64>,
    pub retransmissions: Option<u32>,
    pub congestion_marks: Option<u32>,
    pub pacing_interval_us: Option<u64>,
}

impl TransportMetricsTapEvent {
    pub fn from_tap_pair(main: TapEvent, extension: Option<TapEvent>) -> Option<Self> {
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapEventStage {
    Mint,
    Claim,
    Exhaust,
}

/// Normalised capability lifecycle event (mint → claim → exhaust).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapEvent {
    pub kind_tag: u8,
    pub kind: String,
    pub sid: u32,
    pub stage: CapEventStage,
    pub aux: u32,
}

impl CapEvent {
    #[inline]
    pub fn is_mint(&self) -> bool {
        matches!(self.stage, CapEventStage::Mint)
    }

    #[inline]
    pub fn is_claim(&self) -> bool {
        matches!(self.stage, CapEventStage::Claim)
    }

    #[inline]
    pub fn is_exhaust(&self) -> bool {
        matches!(self.stage, CapEventStage::Exhaust)
    }
}

fn resource_kind_name(tag: u8) -> String {
    match tag {
        EndpointResource::TAG => EndpointResource::NAME.to_string(),
        other => format!("Unknown({other})"),
    }
}

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
        kind: resource_kind_name(tag),
        sid: event.arg0,
        stage,
        aux: event.arg1,
    })
}

/// Extract capability lifecycle events from a tap trace slice.
#[must_use]
pub fn cap_events(events: &[TapEvent]) -> Vec<CapEvent> {
    events.iter().filter_map(decode_cap_event).collect()
}

impl EndpointEvent {
    #[inline]
    pub fn sid(&self) -> u32 {
        match *self {
            EndpointEvent::Send { sid, .. }
            | EndpointEvent::Recv { sid, .. }
            | EndpointEvent::Control { sid, .. } => sid,
        }
    }

    #[inline]
    pub fn lane(&self) -> u8 {
        match *self {
            EndpointEvent::Send { lane, .. }
            | EndpointEvent::Recv { lane, .. }
            | EndpointEvent::Control { lane, .. } => lane,
        }
    }

    #[inline]
    pub fn role(&self) -> u8 {
        match *self {
            EndpointEvent::Send { role, .. }
            | EndpointEvent::Recv { role, .. }
            | EndpointEvent::Control { role, .. } => role,
        }
    }

    #[inline]
    pub fn label(&self) -> u8 {
        match *self {
            EndpointEvent::Send { label, .. }
            | EndpointEvent::Recv { label, .. }
            | EndpointEvent::Control { label, .. } => label,
        }
    }

    #[inline]
    pub fn flags(&self) -> u8 {
        match *self {
            EndpointEvent::Send { flags, .. }
            | EndpointEvent::Recv { flags, .. }
            | EndpointEvent::Control { flags, .. } => flags,
        }
    }

    #[inline]
    pub fn scope(&self) -> Option<ScopeTrace> {
        match *self {
            EndpointEvent::Send { scope, .. }
            | EndpointEvent::Recv { scope, .. }
            | EndpointEvent::Control { scope, .. } => scope,
        }
    }

    #[inline]
    pub fn equivalence_key(&self) -> EndpointEquivalenceKey {
        EndpointEquivalenceKey {
            sid: self.sid(),
            lane: self.lane(),
            role: self.role(),
            label: self.label(),
            kind: match *self {
                EndpointEvent::Send { .. } => EndpointEventKind::Send,
                EndpointEvent::Recv { .. } => EndpointEventKind::Recv,
                EndpointEvent::Control { .. } => EndpointEventKind::Control,
            },
        }
    }

    #[inline]
    pub fn sort_key(&self) -> (u8, u32, u8, u8, u8, u8) {
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

/// Describes the kind of endpoint event for equivalence comparisons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndpointEventKind {
    Send,
    Recv,
    Control,
}

impl EndpointEventKind {
    #[cfg(test)]
    #[inline]
    const fn index(self) -> u8 {
        match self {
            EndpointEventKind::Send => 0,
            EndpointEventKind::Recv => 1,
            EndpointEventKind::Control => 2,
        }
    }
}

/// Equivalence key used to compare endpoint traces in CI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EndpointEquivalenceKey {
    pub sid: u32,
    pub lane: u8,
    pub role: u8,
    pub label: u8,
    pub kind: EndpointEventKind,
}

/// Equivalence key used to compare relay and splice traces for observational equivalence.
///
/// Two forwarding operations are considered observationally equivalent if they
/// produce the same `ForwardEquivalenceKey`. This abstracts over implementation
/// details (relay vs splice, specific lanes/roles/labels/generations) to focus
/// on the essential observable behavior: which session is being forwarded.
///
/// # Why session ID only?
///
/// The current definition uses only `sid` because:
/// - Relay and splice operate on different abstraction levels (frames vs generations)
/// - Lane/role/label are frame-specific (relay), generation is splice-specific
/// - Session ID is the only common observable property across both mechanisms
///
/// This enables verification of the fundamental MPST invariant: **relay ≡ splice**
/// (cut elimination / observational equivalence).
///
/// # Example
///
/// ```rust,ignore
/// let relay_key = relay_event.equivalence_key();
/// let splice_key = splice_event.equivalence_key();
///
/// if relay_key == splice_key {
///     println!("Observationally equivalent: both forward session {}", relay_key.sid);
/// }
/// ```
///
/// See `examples/relay_splice_equivalence.rs` for a complete demonstration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ForwardEquivalenceKey {
    pub sid: u32,
}

/// Normalises the tap range `[start, end)` into delegation boundary events.
pub fn delegation_trace(storage: &[TapEvent], start: usize, end: usize) -> Vec<DelegationEvent> {
    let capacity = storage.len();
    debug_assert!(capacity == RING_EVENTS || capacity == RING_BUFFER_SIZE);
    let mut events = Vec::new();
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
            ids::DELEG_ABORT => {
                let sid = current_sid.unwrap_or(0);
                events.push(DelegationEvent::Abort {
                    sid,
                    lane: raw.arg1 as u8,
                    reason: raw.arg0 as u8,
                });
            }
            ids::SLO_BREACH => {
                let sid = current_sid.unwrap_or(0);
                let retry = ((raw.arg1 >> 16) & 0x1) != 0;
                let queue_tail = (raw.arg1 & 0xFFFF) as u16;
                events.push(DelegationEvent::SloBreach {
                    sid,
                    latency_us: raw.arg0,
                    queue_tail,
                    retry,
                });
            }
            _ => {}
        }
        cursor += 1;
    }
    events
}

/// Extract only the SLO breach events observed in the tap window `[start, end)`.
pub fn slo_breach_trace(storage: &[TapEvent], start: usize, end: usize) -> Vec<SloBreachEvent> {
    delegation_trace(storage, start, end)
        .into_iter()
        .filter_map(|event| match event {
            DelegationEvent::SloBreach {
                sid,
                latency_us,
                queue_tail,
                retry,
            } => Some(SloBreachEvent {
                sid,
                latency_us,
                queue_tail,
                retry,
            }),
            _ => None,
        })
        .collect()
}

/// Normalises the tap range `[start, end)` into forwarder events, collapsing
/// splice begin/commit pairs into a single logical action.
///
/// This function provides the normalization layer that enables observational
/// equivalence checking between relay and splice forwarding mechanisms.
///
/// # Normalization rules
///
/// - **RELAY_FORWARD event** → `ForwardEvent::Relay { sid, lane, role, label, flags }`
/// - **SPLICE_BEGIN + SPLICE_COMMIT pair** → `ForwardEvent::Splice { sid, generation }`
/// - **Unpaired SPLICE_BEGIN** → `ForwardEvent::Splice { sid, generation: arg1 }`
///
/// # Parameters
///
/// - `storage`: The tap event ring buffer (typically from `Rendezvous::tap().as_slice()`)
/// - `start`: Starting cursor position (typically from `tap.head()` before operations)
/// - `end`: Ending cursor position (typically from `tap.head()` after operations)
///
/// # Requires `std` feature
///
/// This function is only available with the `std` feature enabled, as it depends
/// on dynamic allocation via `Vec` for trace normalisation.
///
/// # Example
///
/// ```rust,ignore
/// let start = rendezvous.tap().head();
///
/// // Perform relay operations
/// forward.relay(frame1).await?;
/// forward.relay(frame2).await?;
///
/// let end = rendezvous.tap().head();
/// let trace = normalise::forward_trace(rendezvous.tap().as_slice(), start, end);
///
/// assert_eq!(trace.len(), 2);
/// assert!(matches!(trace[0], ForwardEvent::Relay { .. }));
/// assert!(matches!(trace[1], ForwardEvent::Relay { .. }));
/// ```
///
/// See `examples/relay_splice_equivalence.rs` for a complete demonstration of
/// relay ≡ splice observational equivalence verification.
pub fn forward_trace(storage: &[TapEvent], start: usize, end: usize) -> Vec<ForwardEvent> {
    let capacity = storage.len();
    debug_assert!(capacity == RING_EVENTS || capacity == RING_BUFFER_SIZE);
    let mut events = Vec::new();
    let mut cursor = start;
    while cursor < end {
        let raw = storage[cursor % capacity];
        match raw.id {
            ids::RELAY_FORWARD => {
                let packed = raw.arg1;
                let lane = ((packed >> 16) & 0xFF) as u8;
                let role = ((packed >> 24) & 0xFF) as u8;
                let label = ((packed >> 8) & 0xFF) as u8;
                let flags = (packed & 0xFF) as u8;
                events.push(ForwardEvent::Relay {
                    sid: raw.arg0,
                    lane,
                    role,
                    label,
                    flags,
                });
                cursor += 1;
            }
            ids::SPLICE_BEGIN => {
                if cursor + 1 < end {
                    let next = storage[(cursor + 1) % capacity];
                    if next.id == ids::SPLICE_COMMIT
                        && next.arg0 == raw.arg0
                        && next.arg1 == raw.arg1
                    {
                        events.push(ForwardEvent::Splice {
                            sid: raw.arg0,
                            generation: raw.arg1,
                        });
                        cursor += 2;
                        continue;
                    }
                }
                events.push(ForwardEvent::Splice {
                    sid: raw.arg0,
                    generation: raw.arg1,
                });
                cursor += 1;
            }
            ids::SPLICE_COMMIT => {
                // Ignore stray commits; well-formed traces are collapsed above.
                cursor += 1;
            }
            _ => {
                cursor += 1;
            }
        }
    }
    events
}

/// Normalises the tap range `[start, end)` into endpoint events, decoding
/// packed fields for easier comparison across seq/alt/par compositions.
pub fn endpoint_trace(storage: &[TapEvent], start: usize, end: usize) -> Vec<EndpointEvent> {
    let capacity = storage.len();
    debug_assert!(capacity == RING_EVENTS || capacity == RING_BUFFER_SIZE);
    let mut events = Vec::new();
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
pub fn transport_trace(storage: &[TapEvent], start: usize, end: usize) -> Vec<TransportTapEvent> {
    let capacity = storage.len();
    debug_assert!(capacity == RING_EVENTS || capacity == RING_BUFFER_SIZE);
    let mut events = Vec::new();
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
pub fn transport_metrics_trace(
    storage: &[TapEvent],
    start: usize,
    end: usize,
) -> Vec<TransportMetricsTapEvent> {
    let capacity = storage.len();
    debug_assert!(capacity == RING_EVENTS || capacity == RING_BUFFER_SIZE);
    let mut events = Vec::new();
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

/// Collect policy events observed between `start` and `end`.
pub fn policy_trace(storage: &[TapEvent], start: usize, end: usize) -> Vec<PolicyEvent> {
    let capacity = storage.len();
    debug_assert!(capacity == RING_EVENTS || capacity == RING_BUFFER_SIZE);
    let mut events = Vec::new();
    let mut cursor = start;
    while cursor < end {
        let raw = storage[cursor % capacity];
        if let Some(event) = PolicyEvent::from_tap(raw) {
            events.push(event);
        }
        cursor += 1;
    }
    events
}

/// Collect cancel begin/ack events observed between `start` and `end`.
pub fn cancel_trace(storage: &[TapEvent], start: usize, end: usize) -> Vec<CancelEvent> {
    let capacity = storage.len();
    debug_assert!(capacity == RING_EVENTS || capacity == RING_BUFFER_SIZE);
    let mut events = Vec::new();
    let mut cursor = start;
    while cursor < end {
        let raw = storage[cursor % capacity];
        match raw.id {
            id if id == ids::CANCEL_BEGIN => events.push(CancelEvent {
                kind: CancelEventKind::Begin,
                sid: raw.arg0,
                lane: raw.arg1,
            }),
            id if id == ids::CANCEL_ACK => events.push(CancelEvent {
                kind: CancelEventKind::Ack,
                sid: raw.arg0,
                lane: raw.arg1,
            }),
            _ => {}
        }
        cursor += 1;
    }
    events
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::observe::{RawEvent, events};
    use crate::transport::{
        TransportAlgorithm, TransportEvent, TransportEventKind, TransportSnapshot,
    };

    #[test]
    fn relay_event_decodes_packed_fields() {
        let mut storage = [TapEvent::default(); RING_EVENTS];
        storage[0] = RawEvent::new(
            0,
            ids::RELAY_FORWARD,
            7,
            ((3u32) << 16) | ((5u32) << 24) | ((9u32) << 8) | 0x12,
        );

        let trace = forward_trace(&storage, 0, 1);
        assert_eq!(trace.len(), 1);
        match trace[0] {
            ForwardEvent::Relay {
                sid,
                lane,
                role,
                label,
                flags,
            } => {
                assert_eq!(sid, 7);
                assert_eq!(lane, 3);
                assert_eq!(role, 5);
                assert_eq!(label, 9);
                assert_eq!(flags, 0x12);
            }
            _ => panic!("expected relay event"),
        }
    }

    #[test]
    fn splice_pair_collapses_to_single_event() {
        let mut storage = [TapEvent::default(); RING_EVENTS];
        storage[0] = events::SpliceBegin::new(0, 42, 77);
        storage[1] = events::SpliceCommit::new(0, 42, 77);

        let trace = forward_trace(&storage, 0, 2);
        assert_eq!(trace.len(), 1);
        assert_eq!(
            trace[0],
            ForwardEvent::Splice {
                sid: 42,
                generation: 77
            }
        );
    }

    #[test]
    fn stray_commit_is_skipped() {
        let mut storage = [TapEvent::default(); RING_EVENTS];
        storage[0] = events::SpliceCommit::new(0, 1, 2);
        let trace = forward_trace(&storage, 0, 1);
        assert!(trace.is_empty());
    }

    #[test]
    fn endpoint_trace_decodes_sends_and_recvs() {
        let mut storage = [TapEvent::default(); RING_EVENTS];
        storage[0] = events::EndpointSend::new(
            0,
            9,
            events::EndpointSend::pack(1, 2, 3, 0xAA),
        );
        storage[1] = events::EndpointRecv::new(
            1,
            9,
            events::EndpointRecv::pack(4, 5, 6, 0x55),
        );
        storage[2] = events::EndpointControl::new(
            2,
            9,
            events::EndpointControl::pack(7, 8, 9, 0x10),
        );

        let events = endpoint_trace(&storage, 0, 3);
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
    }

    #[test]
    fn route_decision_event_decodes() {
        let mut storage = [TapEvent::default(); RING_EVENTS];
        storage[0] = events::RouteDecision::with_causal(
            0,
            TapEvent::make_causal_key(4, 1),
            900,
            ((0x1234u32) << 16) | 0x7,
        )
        .with_arg2(0x8000_0000 | ((0x56u32) << 16) | 0x89);

        let events = delegation_trace(&storage, 0, 1);
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
    }

    #[test]
    fn transport_trace_decodes_ack_and_loss() {
        let mut storage = [TapEvent::default(); RING_EVENTS];
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

        let events = transport_trace(&storage, 0, 2);
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
    }

    #[test]
    fn transport_metrics_trace_decodes_snapshot() {
        let mut storage = [TapEvent::default(); RING_EVENTS];
        let snapshot = TransportSnapshot::new(Some(1500), Some(12))
            .with_srtt(Some(3200))
            .with_congestion_window(Some(64 * 1024))
            .with_in_flight(Some(32 * 1024))
            .with_retransmissions(Some(7))
            .with_congestion_marks(Some(3))
            .with_pacing_interval(Some(500))
            .with_algorithm(Some(TransportAlgorithm::Cubic));
        let payload = snapshot.encode_tap_metrics().expect("metrics encode");
        let (arg0, arg1) = payload.primary;
        storage[0] = events::TransportMetrics::new(0, arg0, arg1);
        if let Some((ext0, ext1)) = payload.extension {
            storage[1] = events::TransportMetricsExt::new(1, ext0, ext1);
        }

        let events = transport_metrics_trace(&storage, 0, 2);
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
    }

    #[test]
    fn transport_metrics_trace_handles_missing_extension() {
        let mut storage = [TapEvent::default(); RING_EVENTS];
        let snapshot = TransportSnapshot::new(None, Some(4))
            .with_srtt(Some(6400))
            .with_congestion_window(Some(8 * 1024))
            .with_in_flight(Some(4 * 1024))
            .with_algorithm(Some(TransportAlgorithm::Reno));
        let payload = snapshot.encode_tap_metrics().expect("metrics encode");
        let (arg0, arg1) = payload.primary;
        storage[0] = events::TransportMetrics::new(0, arg0, arg1);

        let events = transport_metrics_trace(&storage, 0, 1);
        assert_eq!(events.len(), 1);
        let event = events[0];
        assert_eq!(event.algorithm, TransportAlgorithm::Reno);
        assert_eq!(event.retransmissions, None);
        assert_eq!(event.congestion_marks, None);
        assert_eq!(event.pacing_interval_us, None);
    }

    #[test]
    fn endpoint_seq_events_preserve_order() {
        let mut storage = [TapEvent::default(); RING_EVENTS];
        for (idx, label) in [10u8, 11, 12].iter().enumerate() {
            storage[idx] = events::EndpointSend::new(
                idx as u32,
                0x20,
                events::EndpointSend::pack(0, 1, *label, 0),
            );
        }

        let events = endpoint_trace(&storage, 0, 3);
        let mut labels = events.iter().map(|event| event.label()).collect::<Vec<_>>();
        assert_eq!(labels.as_slice(), &[10, 11, 12]);

        // Changing the grouping (simulate reassociation) should yield the
        // same linear order, validating seq associativity at the tap level.
        labels.sort();
        assert_eq!(labels.as_slice(), &[10, 11, 12]);
    }

    #[test]
    fn endpoint_alt_arms_remain_disjoint() {
        let mut left = [TapEvent::default(); RING_EVENTS];
        let mut right = [TapEvent::default(); RING_EVENTS];

        left[0] = events::EndpointSend::new(
            0,
            0x30,
            events::EndpointSend::pack(2, 1, 0x1, 0),
        );
        right[0] = events::EndpointSend::new(
            0,
            0x30,
            events::EndpointSend::pack(3, 1, 0x2, 0),
        );

        let left_events = endpoint_trace(&left, 0, 1);
        let right_events = endpoint_trace(&right, 0, 1);

        let mut combined = left_events
            .iter()
            .chain(right_events.iter())
            .map(EndpointEvent::equivalence_key)
            .collect::<Vec<_>>();
        combined.sort_by_key(|key| (key.sid, key.lane, key.role, key.label, key.kind.index()));

        assert_eq!(combined.len(), 2);
        assert!(combined[0].label != combined[1].label);
    }

    #[test]
    fn endpoint_par_traces_align_after_sorting() {
        let mut seq_storage = [TapEvent::default(); RING_EVENTS];
        let mut interleaved_storage = [TapEvent::default(); RING_EVENTS];

        // Sequential order: left branch (lane 1) followed by right branch (lane 2).
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

        // Interleaved to model parallel interleaving.
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

        let seq_events = endpoint_trace(&seq_storage, 0, 4);
        let interleaved_events = endpoint_trace(&interleaved_storage, 0, 4);

        let mut seq_keys: Vec<_> = seq_events.iter().map(EndpointEvent::sort_key).collect();
        let mut interleaved_keys: Vec<_> = interleaved_events
            .iter()
            .map(EndpointEvent::sort_key)
            .collect();

        seq_keys.sort();
        interleaved_keys.sort();

        assert_eq!(seq_keys, interleaved_keys);
    }

    #[test]
    fn slo_trace_extracts_events() {
        let mut storage = [TapEvent::default(); RING_EVENTS];
        storage[0] = events::SloBreach::new(0, 321, 0x0005);
        let trace = slo_breach_trace(&storage, 0, 1);
        assert_eq!(trace.len(), 1);
        let record = trace[0];
        assert_eq!(record.latency_us, 321);
        assert_eq!(record.queue_tail, 5);
        assert!(!record.retry);
    }

    #[test]
    fn scope_correlation_groups_events() {
        let scope = ScopeTrace::new(3, 9);
        let endpoint_events = vec![
            EndpointEvent::Send {
                sid: 7,
                lane: 1,
                role: 2,
                label: 3,
                flags: 0,
                scope: Some(scope),
            },
            EndpointEvent::Recv {
                sid: 7,
                lane: 1,
                role: 2,
                label: 4,
                flags: 0,
                scope: Some(scope),
            },
        ];
        let spec = policy_event_spec(ids::POLICY_ABORT).expect("policy abort spec");
        let event = PolicyEvent {
            kind: PolicyEventKind::Abort,
            arg0: 1,
            arg1: 2,
            lane: Some(5),
        };
        let record = PolicyLaneRecord {
            spec,
            event,
            lane: Some(5),
            association: None,
            sid_hint: spec.sid_hint(&event),
            sid_match: None,
            scope: Some(scope),
        };

        let buckets =
            correlate_scope_traces(&endpoint_events, &[record.clone()], &[record.clone()]);
        let bucket = buckets.get(&scope).expect("scope bucket");
        assert_eq!(bucket.endpoint.len(), 2);
        assert_eq!(bucket.policy.len(), 1);
        assert_eq!(bucket.mgmt.len(), 1);
    }
}
