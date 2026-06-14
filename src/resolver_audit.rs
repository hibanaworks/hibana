//! Resolver audit helpers owned by hibana core.
//!
//! Resolver audit records route-site decisions without owning resolver inputs.
//! The runtime keeps only the slot boundary and deterministic replay helpers
//! needed by the localside kernel.

use crate::observe::core::TapEvent;

/// Resolver slot identity used by audit events.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ResolverSlot {
    EndpointRx,
    EndpointTx,
    Decision,
}

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
