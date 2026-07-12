use core::task::Poll;

use super::Port;
use crate::{
    global::const_dsl::ScopeId,
    rendezvous::{core::EndpointLeaseRecord, tables::RouteTable},
    transport::Transport,
};

impl<'r, T: Transport + 'r> Port<'r, T> {
    #[inline]
    pub(crate) fn seal_session_membership(&self) {
        EndpointLeaseRecord::seal_session_membership(self.endpoint_lease_storage, self.sid);
    }

    #[inline]
    pub(crate) fn route_table(&self) -> &RouteTable {
        // SAFETY: `routes` points to the rendezvous-local RouteTable bound for
        // this port and outliving every lane port reference.
        unsafe { &*self.routes }
    }

    #[inline]
    fn route_observed_mask(&self, peer: Option<u8>) -> u16 {
        if self.role >= self.role_count || self.role_count > u16::BITS as u8 {
            crate::invariant();
        }
        let mut mask = 1u16 << self.role;
        if let Some(peer) = peer {
            if peer >= self.role_count {
                crate::invariant();
            }
            mask |= 1u16 << peer;
        }
        mask
    }

    #[inline]
    fn attached_session_role_mask(&self) -> u16 {
        if self.role_count == 0 || self.role_count > u16::BITS as u8 {
            crate::invariant();
        }
        let (endpoint_leases, slot_count) = self.endpoint_lease_owner_view();
        let mut mask = 0u16;
        let mut idx = 0usize;
        while idx < slot_count {
            /* SAFETY: the freshly loaded owner view contains `slot_count`
            initialized records. No callback or relocation occurs during this
            bounded scan. */
            let slot = unsafe { (&*endpoint_leases.add(idx)).slot() };
            if slot.is_published() && slot.sid == self.sid {
                if slot.role >= self.role_count {
                    crate::invariant();
                }
                mask |= 1u16 << slot.role;
            }
            idx += 1;
        }
        mask
    }

    #[inline]
    pub(crate) fn begin_route_arm_selection(
        &self,
        scope: ScopeId,
        arm: u8,
        participant_mask: u16,
        frame_target: Option<u8>,
    ) -> bool {
        let owner_role_bit = self.route_observed_mask(None);
        let local_participant_mask = RouteTable::selected_local_participant_mask(
            participant_mask,
            self.attached_session_role_mask(),
            owner_role_bit,
        );
        self.route_table().begin_with_role_count(
            self.sid,
            self.role_count,
            local_participant_mask,
            self.route_observed_mask(frame_target) & local_participant_mask,
            scope,
            arm,
        )
    }

    #[inline]
    pub(crate) fn observe_active_route_arm_selection(
        &self,
        scope: ScopeId,
        arm: u8,
        frame_target: Option<u8>,
    ) -> bool {
        self.route_table().observe_active_with_role_count(
            self.sid,
            self.role_count,
            self.route_observed_mask(frame_target),
            scope,
            arm,
        )
    }

    #[inline]
    pub(crate) fn wake_route_arm_selection_waiters(&self) {
        EndpointLeaseRecord::wake_session_waiters(self.endpoint_lease_storage, self.sid, self.role);
    }

    #[inline]
    pub(crate) fn can_begin_route_arm_selection(&self, scope: ScopeId) -> bool {
        self.route_table().can_begin(self.sid, scope)
    }

    #[inline]
    pub(crate) fn poll_route_arm_selection(&self, scope: ScopeId, role: u8) -> Poll<u8> {
        self.route_table()
            .poll_with_role_count(self.sid, self.role_count, role, scope)
    }

    #[inline]
    pub(crate) fn peek_route_arm_selection(&self, scope: ScopeId, role: u8) -> Option<u8> {
        self.route_table()
            .peek_with_role_count(self.sid, self.role_count, role, scope)
    }

    #[inline]
    pub(crate) fn has_pending_route_arm_selection(&self, scope: ScopeId, role: u8) -> bool {
        self.route_table()
            .has_pending_with_role_count(self.sid, self.role_count, role, scope)
    }
}
