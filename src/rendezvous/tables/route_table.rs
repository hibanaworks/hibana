use super::{
    Cell, MAX_TRACKED_ROLES, PhantomData, Poll, ScopeId, ScopeKind, SessionId, UnsafeCell,
};

// # Unsafe Owner Contract
//
// This fragment owns route-decision table frames and the route-scope head.
// Unsafe operations bind resident storage once, keep route frames in explicit
// free lists, and access table slots only after session/scope validation and
// table-capacity checks performed by this owner.
#[derive(Clone, Copy)]
struct ScopeCoord {
    sid: SessionId,
    scope: ScopeId,
}

impl ScopeCoord {
    #[inline]
    fn from_route(sid: SessionId, scope: ScopeId) -> Self {
        if scope.kind() != Some(ScopeKind::Route) {
            crate::invariant();
        }
        Self { sid, scope }
    }
}

mod entry;
use entry::RouteEntry;

#[derive(Clone, Copy)]
struct RouteFrame {
    sid: SessionId,
    scope: ScopeId,
    entry: RouteEntry,
    next: u16,
}

impl RouteFrame {
    fn assign(coord: ScopeCoord, entry: RouteEntry, next: u16) -> Self {
        Self {
            sid: coord.sid,
            scope: coord.scope,
            entry,
            next,
        }
    }

    fn free(next: u16) -> Self {
        Self {
            sid: SessionId::new(0),
            scope: ScopeId::none(),
            entry: RouteEntry::EMPTY,
            next,
        }
    }
}

#[derive(Clone, Copy)]
struct RouteTableStorageParts {
    frames: *mut RouteFrame,
    active_head: *mut u16,
    free_head: *mut u16,
}

#[derive(Clone, Copy)]
struct RouteTableStorageShape {
    route_slots: usize,
}

#[derive(Clone, Copy)]
struct RouteTableStorageBinding {
    parts: RouteTableStorageParts,
    shape: RouteTableStorageShape,
}

pub(crate) struct RouteTable {
    frames: UnsafeCell<*mut RouteFrame>,
    route_slots: Cell<u16>,
    active_head: UnsafeCell<*mut u16>,
    free_head: UnsafeCell<*mut u16>,
    _no_send_sync: PhantomData<*mut ()>,
}

mod storage;

#[cfg(kani)]
mod kani;

impl RouteTable {
    #[inline]
    pub(crate) fn selected_local_participant_mask(
        selected_participants: u16,
        attached_roles: u16,
        owner_role_bit: u16,
    ) -> u16 {
        if owner_role_bit == 0
            || (owner_role_bit & (owner_role_bit - 1)) != 0
            || (selected_participants & owner_role_bit) == 0
            || (attached_roles & owner_role_bit) == 0
        {
            crate::invariant();
        }
        selected_participants & attached_roles
    }

    #[inline]
    fn role_slot_count(role_count: u8) -> usize {
        let role_slots = role_count as usize;
        if role_slots == 0 || role_slots > MAX_TRACKED_ROLES {
            crate::invariant();
        }
        role_slots
    }

    #[inline]
    fn checked_role_bit(role_slots: usize, role: u8) -> u16 {
        let role_idx = role as usize;
        if role_idx >= role_slots || role_idx >= u16::BITS as usize {
            crate::invariant();
        }
        1u16 << (role_idx as u32)
    }

    #[inline]
    fn complete_observed_mask(role_slots: usize) -> u16 {
        if role_slots >= u16::BITS as usize {
            u16::MAX
        } else {
            (1u16 << role_slots) - 1
        }
    }

    #[inline]
    fn initial_observed_mask(role_slots: usize, participant_mask: u16, observed_mask: u16) -> u16 {
        let complete = Self::complete_observed_mask(role_slots);
        if participant_mask == 0
            || (participant_mask & !complete) != 0
            || (observed_mask & !participant_mask) != 0
        {
            crate::invariant();
        }
        (!participant_mask & complete) | observed_mask
    }

    #[inline]
    fn frame_ref(&self, idx: usize) -> &RouteFrame {
        if idx >= self.route_slots() {
            crate::invariant();
        }
        /* SAFETY: `RouteTable` owns the initialized `frames` column; `idx` is
        inside `route_slots`, and callers only hold table-local borrows while
        walking or mutating one route-frame list. */
        unsafe { &*self.frames_ptr().add(idx) }
    }

    #[inline]
    fn frame_ptr_checked(&self, idx: usize) -> *mut RouteFrame {
        if idx >= self.route_slots() {
            crate::invariant();
        }
        self.frames_ptr().wrapping_add(idx)
    }

    #[inline]
    fn with_frame_mut<R>(&self, idx: usize, f: impl FnOnce(&mut RouteFrame) -> R) -> R {
        /* SAFETY: `frame_ptr_checked` bounds `idx` inside the initialized
        route-frame column; the mutable frame reference is scoped to this owner
        callback and cannot escape the route-table method. */
        unsafe { f(&mut *self.frame_ptr_checked(idx)) }
    }

    #[inline]
    fn observe_frame_entry(&self, idx: usize, observed_mask: u16, arm: u8) {
        /* SAFETY: `idx` bounds one initialized owner-exclusive frame with no mutable alias. */
        let entry = unsafe { &mut (*self.frame_ptr_checked(idx)).entry };
        *entry = crate::invariant_some(entry.try_observe(observed_mask, arm));
    }

    #[inline]
    fn mark_unseen_role(&self, idx: usize, role_bit: u16) -> Option<u8> {
        /* SAFETY: `idx` is inside the table-owned initialized frame column.
        The mutable entry borrow records exactly one role observation. */
        let entry = unsafe { &mut (*self.frame_ptr_checked(idx)).entry };
        let (next, arm) = entry.try_consume_role(role_bit)?;
        *entry = next;
        Some(arm)
    }

    #[inline]
    fn active_head(&self) -> u16 {
        /* SAFETY: `active_head` is the initialized list root owned by this
        `RouteTable`; local table access excludes a mutable alias. */
        unsafe { *self.active_head_ptr() }
    }

    #[inline]
    fn set_active_head(&self, head: u16) {
        /* SAFETY: this table owner holds the initialized active-list root and
        excludes aliases at its sole publication point. */
        unsafe {
            *self.active_head_ptr() = head;
        }
    }

    #[inline]
    fn slot_for_scope(&self, coord: ScopeCoord) -> Option<usize> {
        let mut current = self.active_head();
        while current != Self::FRAME_LIST_END {
            let idx = current as usize;
            let frame = self.frame_ref(idx);
            if frame.sid == coord.sid && frame.scope == coord.scope {
                return Some(idx);
            }
            current = frame.next;
        }
        None
    }

    fn publish_slot(&self, coord: ScopeCoord, entry: RouteEntry) {
        let idx = crate::invariant_some(self.pop_free_slot());
        let head = self.active_head();
        self.with_frame_mut(idx, |frame| *frame = RouteFrame::assign(coord, entry, head));
        self.set_active_head(idx as u16);
    }

    fn reclaim_completed_route_slot(&self, slot_idx: usize, role_slots: usize) {
        let role_mask = Self::complete_observed_mask(role_slots);
        if (self.frame_ref(slot_idx).entry.observed_mask & role_mask) != role_mask {
            return;
        }
        let mut prev = Self::FRAME_LIST_END;
        let mut current = self.active_head();
        while current != Self::FRAME_LIST_END {
            let current_idx = current as usize;
            let next = self.frame_ref(current_idx).next;
            if current_idx == slot_idx {
                if prev == Self::FRAME_LIST_END {
                    self.set_active_head(next);
                } else {
                    self.with_frame_mut(prev as usize, |frame| frame.next = next);
                }
                self.push_free_slot(slot_idx);
                return;
            }
            prev = current;
            current = next;
        }
        crate::invariant();
    }

    pub(crate) fn begin_with_role_count(
        &self,
        sid: SessionId,
        role_count: u8,
        participant_mask: u16,
        observed_mask: u16,
        scope: ScopeId,
        arm: u8,
    ) -> bool {
        if arm > 1 {
            crate::invariant();
        }
        let role_slots = Self::role_slot_count(role_count);
        let initial_observed =
            Self::initial_observed_mask(role_slots, participant_mask, observed_mask);
        let coord = ScopeCoord::from_route(sid, scope);
        if self.slot_for_scope(coord).is_some() {
            crate::invariant();
        }
        if initial_observed == Self::complete_observed_mask(role_slots) {
            return false;
        }
        let entry = crate::invariant_some(RouteEntry::EMPTY.try_begin(initial_observed, arm));
        self.publish_slot(coord, entry);
        true
    }

    pub(crate) fn observe_active_with_role_count(
        &self,
        sid: SessionId,
        role_count: u8,
        observed_mask: u16,
        scope: ScopeId,
        arm: u8,
    ) -> bool {
        if arm > 1 {
            crate::invariant();
        }
        let role_slots = Self::role_slot_count(role_count);
        let complete = Self::complete_observed_mask(role_slots);
        if observed_mask == 0 || (observed_mask & !complete) != 0 {
            crate::invariant();
        }
        let coord = ScopeCoord::from_route(sid, scope);
        let Some(slot_idx) = self.slot_for_scope(coord) else {
            return false;
        };
        self.observe_frame_entry(slot_idx, observed_mask, arm);
        self.reclaim_completed_route_slot(slot_idx, role_slots);
        true
    }

    pub(crate) fn can_begin(&self, sid: SessionId, scope: ScopeId) -> bool {
        self.slot_for_scope(ScopeCoord::from_route(sid, scope))
            .is_none()
    }

    pub(crate) fn poll_with_role_count(
        &self,
        sid: SessionId,
        role_count: u8,
        role: u8,
        scope: ScopeId,
    ) -> Poll<u8> {
        let role_slots = Self::role_slot_count(role_count);
        let role_bit = Self::checked_role_bit(role_slots, role);
        let coord = ScopeCoord::from_route(sid, scope);
        let Some(slot_idx) = self.slot_for_scope(coord) else {
            return Poll::Pending;
        };
        let Some(arm) = self.mark_unseen_role(slot_idx, role_bit) else {
            return Poll::Pending;
        };
        self.reclaim_completed_route_slot(slot_idx, role_slots);
        Poll::Ready(arm)
    }

    pub(crate) fn peek_with_role_count(
        &self,
        sid: SessionId,
        role_count: u8,
        role: u8,
        scope: ScopeId,
    ) -> Option<u8> {
        let role_slots = Self::role_slot_count(role_count);
        let role_bit = Self::checked_role_bit(role_slots, role);
        let coord = ScopeCoord::from_route(sid, scope);
        let slot_idx = self.slot_for_scope(coord)?;
        let entry = self.frame_ref(slot_idx).entry;
        (entry.observed_mask != 0 && (entry.observed_mask & role_bit) == 0).then_some(entry.arm)
    }

    pub(crate) fn has_pending_with_role_count(
        &self,
        sid: SessionId,
        role_count: u8,
        role: u8,
        scope: ScopeId,
    ) -> bool {
        let role_slots = Self::role_slot_count(role_count);
        let role_bit = Self::checked_role_bit(role_slots, role);
        let coord = ScopeCoord::from_route(sid, scope);
        let Some(slot_idx) = self.slot_for_scope(coord) else {
            return false;
        };
        let entry = self.frame_ref(slot_idx).entry;
        entry.observed_mask != 0 && (entry.observed_mask & role_bit) == 0
    }

    pub(crate) fn reset_session(&self, sid: SessionId) {
        if self.route_slots() == 0 {
            return;
        }
        let mut prev = Self::FRAME_LIST_END;
        let mut current = self.active_head();
        while current != Self::FRAME_LIST_END {
            let idx = current as usize;
            let next = self.frame_ref(idx).next;
            if self.frame_ref(idx).sid == sid {
                if prev == Self::FRAME_LIST_END {
                    self.set_active_head(next);
                } else {
                    self.with_frame_mut(prev as usize, |frame| frame.next = next);
                }
                self.push_free_slot(idx);
            } else {
                prev = current;
            }
            current = next;
        }
    }
}
