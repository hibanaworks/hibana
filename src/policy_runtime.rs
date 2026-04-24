//! Generic policy runtime helpers retained by hibana core.
//!
//! Phase 6 removes the EPF appliance and management prefixes from core. The
//! runtime keeps only the generic slot boundary and deterministic replay
//! helpers needed by the localside kernel.

use crate::{
    observe::core::TapEvent,
    transport::context::{self, PolicyAttrs},
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

/// Engine-level liveness exhaustion reason for dynamic route decision loops.
pub(crate) const ENGINE_LIVENESS_EXHAUSTED: u16 = 0xFFFE;
/// Audit digest used when hibana core has no installed policy appliance.
pub(crate) const POLICY_DIGEST_NONE: u32 = 0;
/// Audit fuel reading used when hibana core has no installed policy appliance.
pub(crate) const POLICY_FUEL_NONE: u16 = 0;
/// Audit mode tag emitted when hibana core records replay inputs without a local
/// policy appliance.
pub(crate) const POLICY_MODE_AUDIT_ONLY_TAG: u8 = 0;
/// Audit result reason emitted when hibana core has no local policy appliance.
pub(crate) const POLICY_REASON_NO_ENGINE: u16 = 1;

/// Reduced audit verdict domain used by hibana core.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PolicyVerdict {
    NoEngine,
}

#[inline]
pub(crate) const fn verdict_tag(verdict: PolicyVerdict) -> u8 {
    match verdict {
        PolicyVerdict::NoEngine => 0xFF,
    }
}

#[inline]
pub(crate) const fn verdict_arm(verdict: PolicyVerdict) -> u8 {
    let _ = verdict;
    0
}

#[inline]
pub(crate) const fn verdict_reason(verdict: PolicyVerdict) -> u16 {
    match verdict {
        PolicyVerdict::NoEngine => POLICY_REASON_NO_ENGINE,
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

#[inline]
const fn attr_u32(attrs: &PolicyAttrs, id: context::ContextId) -> Option<u32> {
    match attrs.get(id) {
        Some(value) => Some(value.as_u32()),
        None => None,
    }
}

#[inline]
const fn attr_u64(attrs: &PolicyAttrs, id: context::ContextId) -> Option<u64> {
    match attrs.get(id) {
        Some(value) => Some(value.as_u64()),
        None => None,
    }
}

/// Deterministic 32-bit hash of transport attrs attached to policy context.
#[inline]
pub(crate) fn hash_transport_attrs(attrs: &PolicyAttrs) -> u32 {
    let mut hash = FNV32_OFFSET;
    hash = fnv32_mix_opt_u64(hash, attr_u64(attrs, context::core::LATENCY_US));
    hash = fnv32_mix_opt_u32(hash, attr_u32(attrs, context::core::QUEUE_DEPTH));
    hash = fnv32_mix_opt_u64(hash, attr_u64(attrs, context::core::PACING_INTERVAL_US));
    hash = fnv32_mix_opt_u32(hash, attr_u32(attrs, context::core::CONGESTION_MARKS));
    hash = fnv32_mix_opt_u32(hash, attr_u32(attrs, context::core::RETRANSMISSIONS));
    hash = fnv32_mix_opt_u32(hash, attr_u32(attrs, context::core::PTO_COUNT));
    hash = fnv32_mix_opt_u64(hash, attr_u64(attrs, context::core::SRTT_US));
    hash = fnv32_mix_opt_u64(hash, attr_u64(attrs, context::core::LATEST_ACK_PN));
    hash = fnv32_mix_opt_u64(hash, attr_u64(attrs, context::core::CONGESTION_WINDOW));
    hash = fnv32_mix_opt_u64(hash, attr_u64(attrs, context::core::IN_FLIGHT_BYTES));
    match attr_u32(attrs, context::core::TRANSPORT_ALGORITHM) {
        Some(1) => fnv32_mix_u8(hash, 1),
        Some(2) => fnv32_mix_u8(hash, 2),
        Some(raw) if raw >= 0x100 => fnv32_mix_u8(fnv32_mix_u8(hash, 3), (raw - 0x100) as u8),
        Some(raw) => fnv32_mix_u8(fnv32_mix_u8(hash, 3), raw as u8),
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
pub(crate) const fn replay_transport_inputs(attrs: &PolicyAttrs) -> [u32; 4] {
    [
        saturating_u64_to_u32(attr_u64(attrs, context::core::LATENCY_US)),
        opt_u32_or_zero(attr_u32(attrs, context::core::QUEUE_DEPTH)),
        opt_u32_or_zero(attr_u32(attrs, context::core::CONGESTION_MARKS)),
        opt_u32_or_zero(attr_u32(attrs, context::core::RETRANSMISSIONS)),
    ]
}

/// Presence bitmask for replay transport inputs.
#[inline]
pub(crate) const fn replay_transport_presence(attrs: &PolicyAttrs) -> u8 {
    let mut mask = 0u8;
    if attr_u64(attrs, context::core::LATENCY_US).is_some() {
        mask |= 1 << 0;
    }
    if attr_u32(attrs, context::core::QUEUE_DEPTH).is_some() {
        mask |= 1 << 1;
    }
    if attr_u32(attrs, context::core::CONGESTION_MARKS).is_some() {
        mask |= 1 << 2;
    }
    if attr_u32(attrs, context::core::RETRANSMISSIONS).is_some() {
        mask |= 1 << 3;
    }
    mask
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_core_policy_appliance_constants_are_explicit() {
        assert_eq!(POLICY_DIGEST_NONE, 0);
        assert_eq!(POLICY_FUEL_NONE, 0);
        assert_eq!(POLICY_MODE_AUDIT_ONLY_TAG, 0);
        assert_eq!(verdict_tag(PolicyVerdict::NoEngine), 0xFF);
        assert_eq!(
            verdict_reason(PolicyVerdict::NoEngine),
            POLICY_REASON_NO_ENGINE
        );
    }
}
