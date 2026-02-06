//! Capability-based delegation primitives.
//!
//! Implements CapTable for managing nonce-authenticated capability tokens
//! minted by the rendezvous.

use core::{cell::UnsafeCell, marker::PhantomData};

use super::{
    error::CapError,
    types::{Lane, SessionId},
};
use crate::control::cap::{
    CAP_HANDLE_LEN, CAP_NONCE_LEN, CapShot, CapsMask, EndpointResource, ResourceKind,
    resource_kinds,
};
use crate::global::const_dsl::ScopeId;

/// Maximum number of capability entries.
const CAPS_MAX: usize = 64;

/// Internal capability entry.
#[derive(Clone, Copy, Debug)]
pub struct CapEntry {
    pub(crate) sid: SessionId,
    pub(crate) lane: Lane,
    pub(crate) kind_tag: u8,
    pub(crate) shot: CapShot,
    pub(crate) role: u8,
    pub(crate) consumed: bool,
    pub(crate) nonce: [u8; CAP_NONCE_LEN],
    pub(crate) caps_mask: CapsMask,
    pub(crate) handle: [u8; CAP_HANDLE_LEN],
    pub(crate) scope: Option<ScopeId>,
}

/// Capability table (per-Rendezvous).
///
/// Tracks nonce-minted capability tokens scoped to a rendezvous. Each entry
/// stores the originating session/lane pair, shot discipline, resource tag,
/// and the precomputed capability mask for constant-time authorisation.
pub struct CapTable {
    slots: UnsafeCell<[Option<CapEntry>; CAPS_MAX]>,
    _no_send_sync: PhantomData<*mut ()>,
}

impl Default for CapTable {
    fn default() -> Self {
        Self::new()
    }
}

impl CapTable {
    pub const fn new() -> Self {
        Self {
            slots: UnsafeCell::new([None; CAPS_MAX]),
            _no_send_sync: PhantomData,
        }
    }

    #[inline]
    pub(crate) fn insert_entry(&self, entry: CapEntry) -> Result<(), ()> {
        unsafe {
            let slots = &mut *self.slots.get();
            if let Some(slot) = slots.iter_mut().find(|slot| slot.is_none()) {
                *slot = Some(entry);
                return Ok(());
            }
        }
        Err(())
    }

    /// Constant-time comparison of two 16-byte arrays.
    ///
    /// This prevents timing attacks where an attacker could incrementally
    /// guess nonce bytes by measuring response time differences.
    ///
    /// # Security
    /// - Always compares all 16 bytes, regardless of early mismatches
    /// - Uses bitwise operations to avoid conditional branches
    /// - Compiler fence prevents optimization from breaking constant-time property
    #[inline(never)] // Prevent inlining that might break constant-time guarantee
    fn ct_eq_nonce(a: &[u8; CAP_NONCE_LEN], b: &[u8; CAP_NONCE_LEN]) -> bool {
        let mut diff = 0u8;
        for i in 0..CAP_NONCE_LEN {
            diff |= a[i] ^ b[i];
        }
        // Use volatile read to prevent compiler optimization
        let result = diff == 0;
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
        result
    }

    /// Purge all capabilities for a lane (on release).
    #[inline]
    pub(crate) fn purge_lane(&self, lane: Lane) {
        unsafe {
            let slots = &mut *self.slots.get();
            for slot in slots.iter_mut() {
                if slot.is_some_and(|entry| entry.lane == lane) {
                    *slot = None;
                }
            }
        }
    }

    /// Release a capability entry by nonce (used by CapRegisteredToken Drop).
    ///
    /// This is called automatically when a CapRegisteredToken is dropped,
    /// ensuring RAII-based cleanup of registered capabilities.
    #[inline]
    pub(crate) fn release_by_nonce(&self, nonce: &[u8; CAP_NONCE_LEN]) {
        unsafe {
            let slots = &mut *self.slots.get();
            for slot in slots.iter_mut() {
                if slot.is_some_and(|entry| Self::ct_eq_nonce(&entry.nonce, nonce)) {
                    *slot = None;
                    break;
                }
            }
        }
    }

    pub(crate) fn claim_by_nonce(
        &self,
        nonce: &[u8; CAP_NONCE_LEN],
        sid: SessionId,
        lane: Lane,
        expected_tag: u8,
        expected_shot: CapShot,
        expected_mask: CapsMask,
    ) -> Result<(u8, CapsMask, bool, [u8; CAP_HANDLE_LEN], Option<ScopeId>), CapError> {
        unsafe {
            let slots = &mut *self.slots.get();
            for entry in slots.iter_mut().flatten() {
                if entry.sid != sid || entry.lane != lane {
                    continue;
                }
                if !Self::ct_eq_nonce(&entry.nonce, nonce) {
                    continue;
                }
                if entry.kind_tag != expected_tag {
                    return Err(CapError::Mismatch);
                }
                if entry.shot != expected_shot {
                    return Err(CapError::Mismatch);
                }

                let computed_mask = if expected_tag == EndpointResource::TAG {
                    let mut handle = EndpointResource::decode_handle(entry.handle)
                        .map_err(|_| CapError::Mismatch)?;
                    if handle.sid != sid || handle.lane != lane || handle.role != entry.role {
                        EndpointResource::zeroize(&mut handle);
                        return Err(CapError::Mismatch);
                    }
                    let mask = EndpointResource::caps_mask(&handle);
                    EndpointResource::zeroize(&mut handle);
                    mask
                } else {
                    resource_kinds::caps_mask_from_tag(expected_tag, entry.handle)
                        .map_err(|_| CapError::Mismatch)?
                };

                if entry.caps_mask.bits() != computed_mask.bits() {
                    return Err(CapError::Mismatch);
                }
                if expected_mask.bits() != computed_mask.bits() {
                    return Err(CapError::Mismatch);
                }

                let handle_bytes = entry.handle;

                let scope = entry.scope;
                return match entry.shot {
                    CapShot::One => {
                        if entry.consumed {
                            Err(CapError::Exhausted)
                        } else {
                            entry.consumed = true;
                            Ok((entry.role, computed_mask, true, handle_bytes, scope))
                        }
                    }
                    CapShot::Many => Ok((entry.role, computed_mask, false, handle_bytes, scope)),
                };
            }
        }
        Err(CapError::UnknownToken)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::CpEffect;
    use crate::control::cap::{EndpointHandle, EndpointResource};

    #[test]
    fn claim_by_nonce_returns_caps_mask() {
        let table = CapTable::new();
        let nonce = [0xAB; 16];
        let endpoint = EndpointHandle::new(SessionId::new(7), Lane::new(3), 9);
        let caps = EndpointResource::caps_mask(&endpoint);
        let entry = CapEntry {
            sid: SessionId::new(7),
            lane: Lane::new(3),
            kind_tag: EndpointResource::TAG,
            shot: CapShot::Many,
            role: endpoint.role,
            consumed: false,
            nonce,
            caps_mask: caps,
            handle: EndpointResource::encode_handle(&endpoint),
            scope: None,
        };
        table.insert_entry(entry).expect("insert succeeds");

        let (role, returned_caps, exhausted, handle_bytes, scope) = table
            .claim_by_nonce(
                &nonce,
                SessionId::new(7),
                Lane::new(3),
                EndpointResource::TAG,
                CapShot::Many,
                caps,
            )
            .expect("claim succeeds");

        assert_eq!(role, 9);
        assert!(!exhausted);
        assert!(returned_caps.allows(CpEffect::Checkpoint));
        assert_eq!(handle_bytes, EndpointResource::encode_handle(&endpoint));
        assert!(scope.is_none());
    }

    #[test]
    fn one_shot_exhausts_on_second_claim() {
        let table = CapTable::new();
        let nonce = [0xCD; 16];
        let endpoint = EndpointHandle::new(SessionId::new(8), Lane::new(2), 5);
        let caps = EndpointResource::caps_mask(&endpoint);
        let entry = CapEntry {
            sid: SessionId::new(8),
            lane: Lane::new(2),
            kind_tag: EndpointResource::TAG,
            shot: CapShot::One,
            role: endpoint.role,
            consumed: false,
            nonce,
            caps_mask: caps,
            handle: EndpointResource::encode_handle(&endpoint),
            scope: None,
        };
        table.insert_entry(entry).expect("insert succeeds");

        // First claim succeeds and marks as consumed
        let (_, _, exhausted, _, _) = table
            .claim_by_nonce(
                &nonce,
                SessionId::new(8),
                Lane::new(2),
                EndpointResource::TAG,
                CapShot::One,
                caps,
            )
            .expect("first claim succeeds");
        assert!(exhausted, "One shot should be exhausted after first claim");

        // Second claim fails because entry is consumed
        let result = table.claim_by_nonce(
            &nonce,
            SessionId::new(8),
            Lane::new(2),
            EndpointResource::TAG,
            CapShot::One,
            caps,
        );
        assert!(
            matches!(result, Err(CapError::Exhausted)),
            "second claim should fail with Exhausted for consumed One entry"
        );
    }
}
