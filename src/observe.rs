//! Observability façade exposing the core tap ring and optional tooling.
//!
//! This surface re-exports the no_std core (`observe::core`) and, when the
//! `std` feature is enabled, the normalisation helpers used in tests and host
//! tooling (`observe::normalise`). Tap event identifiers are generated at build
//! time and exposed via [`observe::ids`].

/// Core tap ring and trace storage.
pub mod core;
pub use core::{
    AckCounters, AssociationSnapshot, CancelEvent, CancelEventKind, CancelEvents, CancelTrace,
    FenceCounters, HookRegistration, PolicyEvent, PolicyEventDomain, PolicyEventKind,
    PolicyEventSpec, PolicyEvents, PolicyLaneExpectation, PolicySidHint, PolicyTrace, TapBatch,
    TapEvent, TapEvents, TapHook, TapRing, WaitForNewEvents, WaitForNewUserEvents, WakerSlot,
    TAP_BATCH_MAX_EVENTS, clear_hooks, clear_observe_waker, clear_user_waker, emit, for_each_since,
    head, install_ring, policy_event_spec, push, read_at, read_user_at, register_hooks,
    register_observe_waker, register_user_waker, uninstall_ring, user_head,
};

#[cfg(feature = "std")]
pub use core::global_ring_ptr;

/// Tap event identifiers.
pub mod ids;

/// Tap event builders.
pub mod events;
pub use events::*;

/// Trace validation helpers.
pub mod check;
pub use check::{CheckReport, feed, reset, snapshot};

/// Local action utilities for observe.
pub mod local;
pub use local::LocalActionFailure;

/// Scope trace helpers.
pub mod scope;
pub use scope::{ScopeTrace, tap_scope};

#[inline]
pub(crate) const fn cap_mint_id(tag: u8) -> u16 {
    ids::CAP_MINT_BASE + tag as u16
}

#[inline]
pub(crate) const fn cap_claim_id(tag: u8) -> u16 {
    ids::CAP_CLAIM_BASE + tag as u16
}

#[inline]
pub(crate) const fn cap_exhaust_id(tag: u8) -> u16 {
    ids::CAP_EXHAUST_BASE + tag as u16
}

#[inline]
pub const fn cap_mint<K: crate::control::cap::ResourceKind>() -> u16 {
    cap_mint_id(K::TAG)
}

#[inline]
pub const fn cap_claim<K: crate::control::cap::ResourceKind>() -> u16 {
    cap_claim_id(K::TAG)
}

#[inline]
pub const fn cap_exhaust<K: crate::control::cap::ResourceKind>() -> u16 {
    cap_exhaust_id(K::TAG)
}

#[inline]
pub const fn policy_abort() -> u16 {
    ids::POLICY_ABORT
}

#[inline]
pub const fn policy_annot() -> u16 {
    ids::POLICY_ANNOT
}

#[inline]
pub const fn policy_trap() -> u16 {
    ids::POLICY_TRAP
}

#[inline]
pub const fn policy_effect() -> u16 {
    ids::POLICY_EFFECT
}

#[inline]
pub const fn policy_effect_ok() -> u16 {
    ids::POLICY_RA_OK
}

#[inline]
pub const fn policy_commit() -> u16 {
    ids::POLICY_COMMIT
}

#[inline]
pub const fn policy_rollback() -> u16 {
    ids::POLICY_ROLLBACK
}


#[cfg(feature = "std")]
pub mod facet;

#[cfg(feature = "std")]
pub mod normalise;
