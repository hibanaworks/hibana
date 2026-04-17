//! Generic policy runtime helpers retained by hibana core.
//!
//! Phase 6 removes the EPF appliance and management prefixes from core. The
//! runtime keeps only the generic slot boundary, policy-action normalisation,
//! and deterministic replay helpers needed by the localside kernel.

use crate::{
    control::cap::mint::CapsMask,
    control::types::{Lane, SessionId},
    observe::core::TapEvent,
    transport::TransportSnapshot,
};

/// Generic policy slot identity used by resolver/policy seams.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolicySlot {
    Forward,
    EndpointRx,
    EndpointTx,
    Rendezvous,
    Route,
}

/// Trap reasons surfaced in audit trails when a downstream appliance fails
/// closed. Core itself does not execute a policy VM.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Trap {
    FuelExhausted,
    IllegalOpcode(u8),
    OutOfBounds,
    IllegalSyscall,
    VerifyFailed,
}

/// Trap reasons surfaced in audit trails when a downstream appliance fails
/// closed. Core itself does not execute a policy VM.
#[cfg(not(test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Trap {}

/// Abort outcome emitted by a downstream policy appliance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AbortInfo {
    pub(crate) reason: u16,
    pub(crate) trap: Option<Trap>,
}

/// Engine-level fail-closed reason used when policy execution cannot produce
/// a safe decision.
pub(crate) const ENGINE_FAIL_CLOSED: u16 = 0xFFFF;
/// Engine-level liveness exhaustion reason for dynamic route decision loops.
pub(crate) const ENGINE_LIVENESS_EXHAUSTED: u16 = 0xFFFE;

/// Runtime policy mode retained for audit metadata.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PolicyMode {
    Shadow,
    Enforce,
}

/// Runtime policy mode retained for audit metadata.
#[cfg(not(test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PolicyMode {
    Enforce,
}

/// Reduced emergency-plane verdict domain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PolicyVerdict {
    Proceed,
    RouteArm(u8),
    Reject(u16),
}

#[inline]
#[cfg(test)]
pub(crate) const fn policy_mode_tag(mode: PolicyMode) -> u8 {
    match mode {
        PolicyMode::Shadow => 0,
        PolicyMode::Enforce => 1,
    }
}

#[inline]
#[cfg(not(test))]
pub(crate) const fn policy_mode_tag(_mode: PolicyMode) -> u8 {
    1
}

#[inline]
pub(crate) const fn verdict_tag(verdict: PolicyVerdict) -> u8 {
    match verdict {
        PolicyVerdict::Proceed => 0,
        PolicyVerdict::RouteArm(_) => 1,
        PolicyVerdict::Reject(_) => 2,
    }
}

#[inline]
pub(crate) const fn verdict_arm(verdict: PolicyVerdict) -> u8 {
    match verdict {
        PolicyVerdict::RouteArm(arm) => arm,
        _ => 0,
    }
}

#[inline]
pub(crate) const fn verdict_reason(verdict: PolicyVerdict) -> u16 {
    match verdict {
        PolicyVerdict::Reject(reason) => reason,
        _ => 0,
    }
}

#[inline]
pub(crate) const fn slot_tag(slot: PolicySlot) -> u8 {
    match slot {
        PolicySlot::Forward => 0,
        PolicySlot::EndpointRx => 1,
        PolicySlot::EndpointTx => 2,
        PolicySlot::Rendezvous => 3,
        PolicySlot::Route => 4,
    }
}

const FNV32_OFFSET: u32 = 0x811C_9DC5;
const FNV32_PRIME: u32 = 0x0100_0193;

#[inline]
fn fnv32_mix_u8(mut hash: u32, byte: u8) -> u32 {
    hash ^= byte as u32;
    hash.wrapping_mul(FNV32_PRIME)
}

#[inline]
fn fnv32_mix_u16(hash: u32, value: u16) -> u32 {
    let bytes = value.to_le_bytes();
    let hash = fnv32_mix_u8(hash, bytes[0]);
    fnv32_mix_u8(hash, bytes[1])
}

#[inline]
fn fnv32_mix_u32(hash: u32, value: u32) -> u32 {
    let bytes = value.to_le_bytes();
    let hash = fnv32_mix_u8(hash, bytes[0]);
    let hash = fnv32_mix_u8(hash, bytes[1]);
    let hash = fnv32_mix_u8(hash, bytes[2]);
    fnv32_mix_u8(hash, bytes[3])
}

#[inline]
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

#[inline]
fn fnv32_mix_opt_u32(hash: u32, value: Option<u32>) -> u32 {
    match value {
        Some(v) => fnv32_mix_u32(fnv32_mix_u8(hash, 1), v),
        None => fnv32_mix_u8(hash, 0),
    }
}

#[inline]
fn fnv32_mix_opt_u64(hash: u32, value: Option<u64>) -> u32 {
    match value {
        Some(v) => fnv32_mix_u64(fnv32_mix_u8(hash, 1), v),
        None => fnv32_mix_u8(hash, 0),
    }
}

/// Deterministic 32-bit hash of tap input consumed by policy audit replay.
#[inline]
pub(crate) fn hash_tap_event(event: &TapEvent) -> u32 {
    let mut hash = FNV32_OFFSET;
    hash = fnv32_mix_u32(hash, event.ts);
    hash = fnv32_mix_u16(hash, event.id);
    hash = fnv32_mix_u16(hash, event.causal_key);
    hash = fnv32_mix_u32(hash, event.arg0);
    hash = fnv32_mix_u32(hash, event.arg1);
    fnv32_mix_u32(hash, event.arg2)
}

/// Deterministic 32-bit hash of policy input args.
#[inline]
pub(crate) fn hash_policy_input(input: [u32; 4]) -> u32 {
    let mut hash = FNV32_OFFSET;
    let mut idx = 0usize;
    while idx < input.len() {
        hash = fnv32_mix_u32(hash, input[idx]);
        idx += 1;
    }
    hash
}

/// Deterministic 32-bit hash of transport snapshot attached to policy context.
#[inline]
pub(crate) fn hash_transport_snapshot(snapshot: TransportSnapshot) -> u32 {
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
        Some(crate::transport::TransportAlgorithm::Cubic) => fnv32_mix_u8(hash, 1),
        Some(crate::transport::TransportAlgorithm::Reno) => fnv32_mix_u8(hash, 2),
        Some(crate::transport::TransportAlgorithm::Other(code)) => {
            fnv32_mix_u8(fnv32_mix_u8(hash, 3), code)
        }
        None => fnv32_mix_u8(hash, 0),
    }
}

#[inline]
const fn saturating_u64_to_u32(value: Option<u64>) -> u32 {
    match value {
        Some(v) => {
            if v > u32::MAX as u64 {
                u32::MAX
            } else {
                v as u32
            }
        }
        None => 0,
    }
}

#[inline]
const fn opt_u32_or_zero(value: Option<u32>) -> u32 {
    match value {
        Some(v) => v,
        None => 0,
    }
}

/// Canonical replay transport inputs consumed by audit tools.
#[inline]
pub(crate) const fn replay_transport_inputs(snapshot: TransportSnapshot) -> [u32; 4] {
    [
        saturating_u64_to_u32(snapshot.latency_us),
        opt_u32_or_zero(snapshot.queue_depth),
        opt_u32_or_zero(snapshot.congestion_marks),
        opt_u32_or_zero(snapshot.retransmissions),
    ]
}

/// Presence bitmask for replay transport inputs.
#[inline]
pub(crate) const fn replay_transport_presence(snapshot: TransportSnapshot) -> u8 {
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

/// Unified action surface consumed by slot owners.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Proceed,
    Abort(AbortInfo),
    Tap { id: u16, arg0: u32, arg1: u32 },
    Route { arm: u8 },
    Defer { retry_hint: u8 },
}

/// Unified action surface consumed by slot owners.
#[cfg(not(test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Proceed,
}

impl Action {
    #[inline]
    #[cfg(test)]
    pub(crate) const fn verdict(self) -> PolicyVerdict {
        match self {
            Action::Proceed => PolicyVerdict::Proceed,
            Action::Route { arm } if arm <= 1 => PolicyVerdict::RouteArm(arm),
            Action::Route { .. } => PolicyVerdict::Reject(ENGINE_FAIL_CLOSED),
            Action::Abort(info) => PolicyVerdict::Reject(info.reason),
            Action::Tap { .. } => PolicyVerdict::Proceed,
            Action::Defer { .. } => PolicyVerdict::Proceed,
        }
    }

    #[inline]
    #[cfg(not(test))]
    pub(crate) const fn verdict(self) -> PolicyVerdict {
        let _ = self;
        let _ = PolicyVerdict::RouteArm(0);
        let _ = PolicyVerdict::Reject(ENGINE_FAIL_CLOSED);
        PolicyVerdict::Proceed
    }

    #[inline]
    #[cfg(test)]
    pub(crate) const fn with_mode(self, mode: PolicyMode) -> Self {
        match mode {
            PolicyMode::Enforce => self,
            PolicyMode::Shadow => match self {
                Action::Tap { .. } => self,
                _ => Action::Proceed,
            },
        }
    }

    #[inline]
    #[cfg(not(test))]
    pub(crate) const fn abort_info(self) -> Option<AbortInfo> {
        let _ = self;
        None
    }

    #[inline]
    #[cfg(test)]
    pub(crate) const fn abort_info(self) -> Option<AbortInfo> {
        match self {
            Action::Abort(info) => Some(info),
            _ => None,
        }
    }

    #[inline]
    #[cfg(not(test))]
    pub(crate) const fn tap_payload(self) -> Option<(u16, u32, u32)> {
        let _ = self;
        None
    }

    #[inline]
    #[cfg(test)]
    pub(crate) const fn tap_payload(self) -> Option<(u16, u32, u32)> {
        match self {
            Action::Tap { id, arg0, arg1 } => Some((id, arg0, arg1)),
            _ => None,
        }
    }

    #[inline]
    #[cfg(not(test))]
    pub(crate) const fn route_arm(self) -> Option<u8> {
        let _ = self;
        None
    }

    #[inline]
    #[cfg(test)]
    pub(crate) const fn route_arm(self) -> Option<u8> {
        match self {
            Action::Route { arm } => Some(arm),
            _ => None,
        }
    }

    #[inline]
    #[cfg(not(test))]
    pub(crate) const fn defer_hint(self) -> Option<u8> {
        let _ = self;
        None
    }

    #[inline]
    #[cfg(test)]
    pub(crate) const fn defer_hint(self) -> Option<u8> {
        match self {
            Action::Defer { retry_hint } => Some(retry_hint),
            _ => None,
        }
    }
}

/// Static contract associated with each policy slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SlotPolicyContract {
    pub(crate) allows_get_input: bool,
    pub(crate) allows_attr: bool,
    pub(crate) allows_mem_ops: bool,
    pub(crate) source: SlotPolicySource,
}

/// Policy signal source associated with a slot contract.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SlotPolicySource {
    Binding,
    Zero,
}

impl SlotPolicyContract {
    const fn new(
        allows_get_input: bool,
        allows_attr: bool,
        allows_mem_ops: bool,
        source: SlotPolicySource,
    ) -> Self {
        Self {
            allows_get_input,
            allows_attr,
            allows_mem_ops,
            source,
        }
    }
}

#[inline]
pub(crate) const fn slot_policy_contract(slot: PolicySlot) -> SlotPolicyContract {
    match slot {
        PolicySlot::Route | PolicySlot::EndpointTx | PolicySlot::EndpointRx => {
            SlotPolicyContract::new(
                true,
                true,
                !matches!(slot, PolicySlot::Route),
                SlotPolicySource::Binding,
            )
        }
        PolicySlot::Forward | PolicySlot::Rendezvous => {
            SlotPolicyContract::new(false, false, true, SlotPolicySource::Zero)
        }
    }
}

#[inline]
pub(crate) const fn slot_default_input(slot: PolicySlot) -> [u32; 4] {
    match slot_policy_contract(slot).source {
        SlotPolicySource::Binding => [0; 4],
        SlotPolicySource::Zero => [0; 4],
    }
}

/// Minimal policy context retained by core so call sites can continue to seed
/// deterministic audit metadata even when no appliance is installed.
#[derive(Debug, Default)]
pub(crate) struct PolicyCtx<'a> {
    _event: core::marker::PhantomData<&'a TapEvent>,
    _slot: Option<PolicySlot>,
    _caps: Option<CapsMask>,
    _session: Option<SessionId>,
    _lane: Option<Lane>,
    _transport: TransportSnapshot,
    _input: [u32; 4],
}

impl<'a> PolicyCtx<'a> {
    #[inline]
    pub(crate) fn new(slot: PolicySlot, _event: &'a TapEvent, caps: CapsMask) -> Self {
        Self {
            _event: core::marker::PhantomData,
            _slot: Some(slot),
            _caps: Some(caps),
            _session: None,
            _lane: None,
            _transport: TransportSnapshot::default(),
            _input: [0; 4],
        }
    }

    #[inline]
    pub(crate) fn set_session(&mut self, session: SessionId) {
        self._session = Some(session);
    }

    #[inline]
    pub(crate) fn set_lane(&mut self, lane: Lane) {
        self._lane = Some(lane);
    }

    #[inline]
    pub(crate) fn set_transport_snapshot(&mut self, snapshot: TransportSnapshot) {
        self._transport = snapshot;
    }

    #[inline]
    pub(crate) fn set_policy_input(&mut self, input: [u32; 4]) {
        self._input = input;
    }
}

/// Placeholder slot registry kept by hibana core after the EPF appliance moved
/// to the sibling crate.
pub(crate) struct HostSlots<'arena> {
    _arena: core::marker::PhantomData<&'arena ()>,
}

impl<'arena> HostSlots<'arena> {
    #[inline]
    pub(crate) fn new() -> Self {
        Self {
            _arena: core::marker::PhantomData,
        }
    }

    pub(crate) unsafe fn init_empty(dst: *mut Self) {
        unsafe {
            core::ptr::addr_of_mut!((*dst)._arena).write(core::marker::PhantomData);
        }
    }

    #[inline]
    pub(crate) fn active_digest(&self, _slot: PolicySlot) -> u32 {
        0
    }

    #[inline]
    pub(crate) fn policy_mode(&self, _slot: PolicySlot) -> PolicyMode {
        PolicyMode::Enforce
    }

    #[inline]
    pub(crate) fn last_fuel_used(&self, _slot: PolicySlot) -> u16 {
        0
    }
}

impl Default for HostSlots<'_> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_helpers_cover_non_proceed_variants() {
        let abort = Action::Abort(AbortInfo {
            reason: 7,
            trap: Some(Trap::VerifyFailed),
        });
        assert_eq!(abort.abort_info().unwrap().reason, 7);

        let tap = Action::Tap {
            id: 3,
            arg0: 4,
            arg1: 5,
        };
        assert_eq!(tap.tap_payload(), Some((3, 4, 5)));

        let route = Action::Route { arm: 1 };
        assert_eq!(route.route_arm(), Some(1));

        let defer = Action::Defer { retry_hint: 9 };
        assert_eq!(defer.defer_hint(), Some(9));
    }

    #[test]
    fn shadow_mode_suppresses_non_tap_actions() {
        assert_eq!(
            Action::Proceed.with_mode(PolicyMode::Shadow),
            Action::Proceed
        );
        assert_eq!(
            Action::Abort(AbortInfo {
                reason: 11,
                trap: Some(Trap::FuelExhausted),
            })
            .with_mode(PolicyMode::Shadow),
            Action::Proceed
        );
        assert_eq!(
            Action::Tap {
                id: 1,
                arg0: 2,
                arg1: 3,
            }
            .with_mode(PolicyMode::Shadow),
            Action::Tap {
                id: 1,
                arg0: 2,
                arg1: 3,
            }
        );
    }

    #[test]
    fn trap_variants_stay_addressable_for_audit_paths() {
        let traps = [
            Trap::FuelExhausted,
            Trap::IllegalOpcode(0xAA),
            Trap::OutOfBounds,
            Trap::IllegalSyscall,
            Trap::VerifyFailed,
        ];
        assert_eq!(traps.len(), 5);
    }
}
