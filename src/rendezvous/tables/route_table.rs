use super::{
    Cell, Lane, MAX_TRACKED_ROLES, PhantomData, Poll, ScopeId, ScopeKind, SessionId, UnsafeCell,
};

// # Unsafe Owner Contract
//
// This fragment owns route-decision table frames and route-scope head columns.
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

#[derive(Clone, Copy)]
struct RouteEntry {
    arm: u8,
    seen_mask: u16,
}

impl RouteEntry {
    const EMPTY: Self = Self {
        arm: 0,
        seen_mask: 0,
    };
}

#[derive(Clone, Copy)]
struct RouteFrame {
    sid: SessionId,
    scope: ScopeId,
    entry: RouteEntry,
    next: u16,
}

impl RouteFrame {
    fn assign(coord: ScopeCoord, next: u16) -> Self {
        Self {
            sid: coord.sid,
            scope: coord.scope,
            entry: RouteEntry::EMPTY,
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
    lane_heads: *mut u16,
    free_head: *mut u16,
}

#[derive(Clone, Copy)]
struct RouteTableStorageShape {
    route_slots: usize,
    lane_base: u32,
    lane_slots: usize,
}

#[derive(Clone, Copy)]
struct RouteTableStorageBinding {
    parts: RouteTableStorageParts,
    shape: RouteTableStorageShape,
}

pub(crate) struct RouteTable {
    frames: UnsafeCell<*mut RouteFrame>,
    route_slots: Cell<u16>,
    lane_base: Cell<u32>,
    lane_slots: Cell<u16>,
    lane_heads: UnsafeCell<*mut u16>,
    free_head: UnsafeCell<*mut u16>,
    _no_send_sync: PhantomData<*mut ()>,
}

mod storage;

impl RouteTable {
    #[inline]
    fn lane_slot(&self, lane: Lane) -> usize {
        if lane.raw() < self.lane_base.get() {
            crate::invariant();
        }
        let lane_idx = (lane.raw() - self.lane_base.get()) as usize;
        if lane_idx >= self.lane_slots() {
            crate::invariant();
        }
        lane_idx
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
    fn complete_seen_mask(role_slots: usize) -> u16 {
        if role_slots >= u16::BITS as usize {
            u16::MAX
        } else {
            (1u16 << role_slots) - 1
        }
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
    fn record_frame_entry(&self, idx: usize, role_bit: u16, arm: u8) {
        /* SAFETY: `idx` is a route frame owned by this table. This method is
        the sole publication point for one route decision. */
        let entry = unsafe { &mut (*self.frame_ptr_checked(idx)).entry };
        entry.arm = arm;
        entry.seen_mask = role_bit;
    }

    #[inline]
    fn mark_unseen_role(&self, idx: usize, role_bit: u16) -> Option<u8> {
        /* SAFETY: `idx` is inside the table-owned initialized frame column.
        The mutable entry borrow records exactly one role observation. */
        let entry = unsafe { &mut (*self.frame_ptr_checked(idx)).entry };
        if entry.seen_mask == 0 || (entry.seen_mask & role_bit) != 0 {
            return None;
        }
        entry.seen_mask |= role_bit;
        Some(entry.arm)
    }

    #[inline]
    fn lane_head(&self, lane_idx: usize) -> u16 {
        if lane_idx >= self.lane_slots() {
            crate::invariant();
        }
        /* SAFETY: `lane_idx` is inside the initialized lane-head column owned
        by this `RouteTable`; local table access excludes a mutable alias. */
        unsafe { *self.lane_heads_ptr().add(lane_idx) }
    }

    #[inline]
    fn set_lane_head(&self, lane_idx: usize, head: u16) {
        if lane_idx >= self.lane_slots() {
            crate::invariant();
        }
        /* SAFETY: this table owns the lane-head column and this mutation is the
        single route-list head update for `lane_idx`. */
        unsafe {
            *self.lane_heads_ptr().add(lane_idx) = head;
        }
    }

    #[inline]
    fn slot_for_scope(&self, lane_idx: usize, coord: ScopeCoord) -> Option<usize> {
        let mut current = self.lane_head(lane_idx);
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

    fn slot_or_alloc(&self, lane_idx: usize, coord: ScopeCoord) -> usize {
        if let Some(idx) = self.slot_for_scope(lane_idx, coord) {
            return idx;
        }
        let idx = crate::invariant_some(self.pop_free_slot());
        let head = self.lane_head(lane_idx);
        self.with_frame_mut(idx, |frame| *frame = RouteFrame::assign(coord, head));
        self.set_lane_head(lane_idx, idx as u16);
        idx
    }

    fn reclaim_completed_route_slot(&self, lane_idx: usize, slot_idx: usize, role_slots: usize) {
        let role_mask = Self::complete_seen_mask(role_slots);
        if (self.frame_ref(slot_idx).entry.seen_mask & role_mask) != role_mask {
            return;
        }
        let mut prev = Self::FRAME_LIST_END;
        let mut current = self.lane_head(lane_idx);
        while current != Self::FRAME_LIST_END {
            let current_idx = current as usize;
            let next = self.frame_ref(current_idx).next;
            if current_idx == slot_idx {
                if prev == Self::FRAME_LIST_END {
                    self.set_lane_head(lane_idx, next);
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

    pub(crate) fn record_with_role_count(
        &self,
        sid: SessionId,
        lane: Lane,
        role_count: u8,
        role_from: u8,
        scope: ScopeId,
        arm: u8,
    ) {
        if arm > 1 {
            crate::invariant();
        }
        let role_slots = Self::role_slot_count(role_count);
        let role_bit = Self::checked_role_bit(role_slots, role_from);
        let coord = ScopeCoord::from_route(sid, scope);
        let lane_idx = self.lane_slot(lane);
        let slot_idx = self.slot_or_alloc(lane_idx, coord);
        self.record_frame_entry(slot_idx, role_bit, arm);
    }

    pub(crate) fn poll_with_role_count(
        &self,
        sid: SessionId,
        lane: Lane,
        role_count: u8,
        role: u8,
        scope: ScopeId,
    ) -> Poll<u8> {
        let role_slots = Self::role_slot_count(role_count);
        let role_bit = Self::checked_role_bit(role_slots, role);
        let coord = ScopeCoord::from_route(sid, scope);
        let lane_idx = self.lane_slot(lane);
        let slot_idx = self.slot_or_alloc(lane_idx, coord);
        let Some(arm) = self.mark_unseen_role(slot_idx, role_bit) else {
            return Poll::Pending;
        };
        self.reclaim_completed_route_slot(lane_idx, slot_idx, role_slots);
        Poll::Ready(arm)
    }

    pub(crate) fn peek_with_role_count(
        &self,
        sid: SessionId,
        lane: Lane,
        role_count: u8,
        role: u8,
        scope: ScopeId,
    ) -> Option<u8> {
        let role_slots = Self::role_slot_count(role_count);
        let role_bit = Self::checked_role_bit(role_slots, role);
        let coord = ScopeCoord::from_route(sid, scope);
        let lane_idx = self.lane_slot(lane);
        let slot_idx = self.slot_for_scope(lane_idx, coord)?;
        let entry = self.frame_ref(slot_idx).entry;
        (entry.seen_mask != 0 && (entry.seen_mask & role_bit) == 0).then_some(entry.arm)
    }

    pub(crate) fn has_pending_lane_with_role_count(
        &self,
        sid: SessionId,
        role_count: u8,
        role: u8,
        scope: ScopeId,
        lane: Lane,
    ) -> bool {
        let role_slots = Self::role_slot_count(role_count);
        let role_bit = Self::checked_role_bit(role_slots, role);
        let coord = ScopeCoord::from_route(sid, scope);
        let lane_idx = self.lane_slot(lane);
        let Some(slot_idx) = self.slot_for_scope(lane_idx, coord) else {
            return false;
        };
        let entry = self.frame_ref(slot_idx).entry;
        entry.seen_mask != 0 && (entry.seen_mask & role_bit) == 0
    }

    pub(crate) fn reset_session_lane(&self, sid: SessionId, lane: Lane) {
        if self.route_slots() == 0 {
            return;
        }
        let lane_idx = self.lane_slot(lane);
        let mut prev = Self::FRAME_LIST_END;
        let mut current = self.lane_head(lane_idx);
        while current != Self::FRAME_LIST_END {
            let idx = current as usize;
            let next = self.frame_ref(idx).next;
            if self.frame_ref(idx).sid == sid {
                if prev == Self::FRAME_LIST_END {
                    self.set_lane_head(lane_idx, next);
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

    pub(crate) fn reset_session(&self, sid: SessionId) {
        let mut lane_idx = 0usize;
        while lane_idx < self.lane_slots() {
            let lane_raw = crate::invariant_some(
                self.lane_base
                    .get()
                    .checked_add(crate::invariant_ok(u32::try_from(lane_idx))),
            );
            self.reset_session_lane(sid, Lane::new(lane_raw));
            lane_idx += 1;
        }
    }
}
