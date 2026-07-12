//! Session-generation program identity recovered from published endpoints.
//!
//! # Unsafe Owner Contract
//!
//! A published endpoint lease owns one fully initialized endpoint image in the
//! rendezvous slab. Program identity may be projected only from that checked
//! range, and every endpoint under one session ID must name the same image.

use super::{Rendezvous, Transport};
use crate::session::types::SessionId;

impl<'rv, 'cfg, T: Transport> Rendezvous<'rv, 'cfg, T>
where
    'cfg: 'rv,
{
    #[inline]
    pub(crate) fn session_membership_is_sealed(&self, sid: SessionId) -> bool {
        let slot_count = self.endpoint_lease_slot_count();
        let mut idx = 0usize;
        while idx < slot_count {
            let slot = crate::invariant_some(self.endpoint_lease_slot_by_index(idx));
            if slot.sid == sid && slot.state.is_membership_sealed() {
                return true;
            }
            idx += 1;
        }
        false
    }

    /// Exact compiled-program binding for one published session generation.
    /// Reserved slots are unreachable because registry access is rejected
    /// while an attach lease is active.
    #[inline]
    pub(crate) fn endpoint_session_program(
        &self,
        sid: SessionId,
    ) -> Option<&'static crate::global::compiled::images::CompiledProgramRef> {
        let (slab_ptr, slab_len) = self.slab_ptr_and_len();
        let slot_count = self.endpoint_lease_slot_count();
        let mut bound: Option<&'static crate::global::compiled::images::CompiledProgramRef> = None;
        let mut idx = 0usize;
        while idx < slot_count {
            let slot = crate::invariant_some(self.endpoint_lease_slot_by_index(idx));
            if !slot.is_occupied() || slot.sid != sid {
                idx += 1;
                continue;
            }
            if !slot.is_published() {
                crate::invariant();
            }
            let offset = slot.offset as usize;
            let len = slot.len as usize;
            let end = crate::invariant_some(offset.checked_add(len));
            if len == 0 || end > slab_len {
                crate::invariant();
            }
            let program = /* SAFETY: this published lease owns an initialized
            endpoint image in the checked slab range. */ unsafe {
                crate::endpoint::kernel::endpoint_init::resident_program_ref::<T>(
                    slab_ptr.add(offset),
                    len,
                )
            };
            if let Some(existing) = bound {
                if !existing.same_image(program) {
                    crate::invariant();
                }
            } else {
                bound = Some(program);
            }
            idx += 1;
        }
        bound
    }
}
