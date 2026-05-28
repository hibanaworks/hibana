//! Generic policy runtime helpers retained by hibana core.
//!
//! Core contains no policy appliance or management-prefix owner. The runtime
//! keeps only the generic slot boundary and deterministic replay helpers needed
//! by the localside kernel.

use crate::{
    observe::core::TapEvent,
    transport::context::{PolicyAttrs, PolicyInput},
};

/// Generic policy slot identity used by resolver/policy seams.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PolicySlot {
    EndpointRx,
    EndpointTx,
    Decision,
}

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
        PolicySlot::EndpointRx => 1,
        PolicySlot::EndpointTx => 2,
        PolicySlot::Decision => 4,
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

/// Deterministic 32-bit hash of resolver-facing policy input.
#[inline]
pub(crate) fn hash_policy_input(input: PolicyInput) -> u32 {
    fnv32_mix_u32(FNV32_OFFSET, input.primary())
}

/// Deterministic 32-bit hash of policy attrs attached to policy context.
#[inline]
pub(crate) fn hash_policy_attrs(attrs: &PolicyAttrs) -> u32 {
    let mut hash = FNV32_OFFSET;
    hash = fnv32_mix_opt_u64(hash, attrs.latency_us());
    fnv32_mix_opt_u32(hash, attrs.queue_depth())
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

/// Canonical replay policy-attribute words consumed by audit tools.
#[inline]
pub(crate) const fn replay_policy_attr_words(attrs: &PolicyAttrs) -> [u32; 4] {
    [
        saturating_u64_to_u32(attrs.latency_us()),
        opt_u32_or_zero(attrs.queue_depth()),
        0,
        0,
    ]
}

/// Presence bitmask for replay policy-attribute words.
#[inline]
pub(crate) const fn replay_policy_attr_presence(attrs: &PolicyAttrs) -> u8 {
    let mut mask = 0u8;
    if attrs.latency_us().is_some() {
        mask |= 1 << 0;
    }
    if attrs.queue_depth().is_some() {
        mask |= 1 << 1;
    }
    mask
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
