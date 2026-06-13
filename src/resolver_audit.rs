//! Resolver audit helpers owned by hibana core.
//!
//! Core contains no resolver appliance or hidden decision owner. The runtime
//! keeps only the slot boundary and deterministic replay helpers needed by the
//! localside kernel.

use crate::observe::core::TapEvent;

/// Generic slot identity used by resolver audit events.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ResolverSlot {
    EndpointRx,
    EndpointTx,
    Decision,
}

/// Audit digest used when hibana core has no installed resolver appliance.
pub(crate) const RESOLVER_DIGEST_NONE: u32 = 0;
/// Audit fuel reading used when hibana core has no installed resolver appliance.
pub(crate) const RESOLVER_FUEL_NONE: u16 = 0;
/// Audit mode tag emitted when hibana core records replay inputs without a local
/// resolver appliance.
pub(crate) const RESOLVER_MODE_AUDIT_ONLY_TAG: u8 = 0;
/// Audit result reason emitted when hibana core has no local resolver appliance.
pub(crate) const RESOLVER_REASON_NO_ENGINE: u16 = 1;
pub(crate) const RESOLVER_RESULT_NO_ENGINE_TAG: u8 = 0xFF;
pub(crate) const RESOLVER_RESULT_NO_ENGINE_ARM: u8 = 0;
pub(crate) const EMPTY_RESOLVER_INPUT_WORDS: [u32; 4] = [0, 0, 0, 0];
pub(crate) const EMPTY_RESOLVER_INPUT_ARG0: u32 = 0;

#[inline]
pub(crate) const fn slot_tag(slot: ResolverSlot) -> u8 {
    match slot {
        ResolverSlot::EndpointRx => 1,
        ResolverSlot::EndpointTx => 2,
        ResolverSlot::Decision => 4,
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

/// Deterministic 32-bit hash of tap input consumed by resolver audit replay.
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

/// Deterministic 32-bit hash of Hibana core's empty resolver-facing audit input.
#[inline]
pub(crate) fn hash_empty_resolver_input() -> u32 {
    fnv32_mix_u32(FNV32_OFFSET, EMPTY_RESOLVER_INPUT_ARG0)
}

/// Deterministic 32-bit hash of the empty core resolver-attribute carrier.
#[inline]
pub(crate) fn hash_empty_resolver_attrs() -> u32 {
    fnv32_mix_u8(FNV32_OFFSET, 0)
}

/// Deterministic 32-bit hash of replayed empty resolver attributes.
#[inline]
pub(crate) fn hash_empty_resolver_replay_attrs() -> u32 {
    let hash = fnv32_mix_opt_u64(FNV32_OFFSET, None);
    fnv32_mix_opt_u32(hash, None)
}

/// Canonical replay resolver-attribute words consumed by audit tools.
pub(crate) const EMPTY_RESOLVER_ATTR_WORDS: [u32; 4] = [0, 0, 0, 0];

/// Presence bitmask for the empty replay resolver-attribute words.
pub(crate) const EMPTY_RESOLVER_ATTR_PRESENCE: u8 = 0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_core_resolver_appliance_constants_are_explicit() {
        assert_eq!(RESOLVER_DIGEST_NONE, 0);
        assert_eq!(RESOLVER_FUEL_NONE, 0);
        assert_eq!(RESOLVER_MODE_AUDIT_ONLY_TAG, 0);
        assert_eq!(RESOLVER_RESULT_NO_ENGINE_TAG, 0xFF);
        assert_eq!(RESOLVER_RESULT_NO_ENGINE_ARM, 0);
        assert_eq!(RESOLVER_REASON_NO_ENGINE, 1);
    }
}
