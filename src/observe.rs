//! Observability surface exposing canonical observe modules.
//!
//! The no_std tap ring lives in [`observe::core`]. Tap event identifiers are
//! generated at build time and consumed internally by the canonical observe
//! owners.

/// Core tap ring and trace storage.
pub(crate) mod core;

/// Tap event identifiers.
pub(crate) mod ids;

/// Tap event builders.
pub(crate) mod events;

/// Scope trace helpers.
pub(crate) mod scope;

#[inline]
const fn cap_mint_id(tag: u8) -> u16 {
    ids::CAP_MINT_BASE + tag as u16
}

#[inline]
const fn cap_claim_id(tag: u8) -> u16 {
    ids::CAP_CLAIM_BASE + tag as u16
}

#[inline]
const fn cap_exhaust_id(tag: u8) -> u16 {
    ids::CAP_EXHAUST_BASE + tag as u16
}

#[inline]
pub(crate) const fn cap_mint<K: crate::control::cap::mint::ResourceKind>() -> u16 {
    cap_mint_id(K::TAG)
}

#[inline]
pub(crate) const fn cap_claim<K: crate::control::cap::mint::ResourceKind>() -> u16 {
    cap_claim_id(K::TAG)
}

#[inline]
pub(crate) const fn cap_exhaust<K: crate::control::cap::mint::ResourceKind>() -> u16 {
    cap_exhaust_id(K::TAG)
}

#[cfg(test)]
mod normalise;
