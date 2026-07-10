use super::{
    Cell, Context, FrameLabelMask, Lane, MAX_TRACKED_ROLES, PhantomData, Poll, ScopeId, ScopeKind,
    UnsafeCell, WaiterSlot,
};
// # Unsafe Owner Contract
//
// This fragment owns route-decision table frames and route-scope head columns.
// Unsafe operations bind resident storage once, keep route frames in explicit
// free lists, and access table slots only after scope validation and
// table-capacity checks performed by this owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ScopeCoord {
    scope: ScopeId,
}

impl ScopeCoord {
    fn from_scope(scope: ScopeId) -> Option<Self> {
        if scope.kind() != Some(ScopeKind::Route) {
            return None;
        }
        Some(Self { scope })
    }
}

#[derive(Clone, Copy)]
struct RouteEntry {
    pub(crate) generation: u16,
    pub(crate) arm: u8,
    seen_mask: u16,
}

impl RouteEntry {
    pub(crate) const fn empty() -> Self {
        Self {
            generation: 0,
            arm: 0,
            seen_mask: 0,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct RouteFrame {
    pub(crate) scope: ScopeId,
    entry: RouteEntry,
    next: u16,
}

impl RouteFrame {
    fn assign(coord: ScopeCoord, next: u16) -> Self {
        Self {
            scope: coord.scope,
            entry: RouteEntry::empty(),
            next,
        }
    }

    pub(crate) fn free(next: u16) -> Self {
        Self {
            scope: ScopeId::none(),
            entry: RouteEntry::empty(),
            next,
        }
    }
}

#[derive(Clone, Copy)]
struct RouteTableStorageParts {
    frames: *mut RouteFrame,
    lane_heads: *mut u16,
    free_head: *mut u16,
    pending_frame_hint_masks: *mut FrameLabelMask,
    waiters: *mut WaiterSlot,
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
    route_slots: Cell<usize>,
    lane_base: Cell<u32>,
    lane_slots: Cell<u16>,
    lane_heads: UnsafeCell<*mut u16>,
    free_head: UnsafeCell<*mut u16>,
    pending_frame_hint_masks: UnsafeCell<*mut FrameLabelMask>,
    waiters: UnsafeCell<*mut WaiterSlot>,
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
        if role_count as usize > MAX_TRACKED_ROLES {
            crate::invariant();
        }
        role_count as usize
    }

    #[inline]
    fn complete_seen_mask(role_slots: usize) -> u16 {
        if role_slots == 0 {
            0
        } else if role_slots >= u16::BITS as usize {
            u16::MAX
        } else {
            (1u16 << role_slots) - 1
        }
    }

    #[inline]
    fn frame_ref(&self, idx: usize) -> &RouteFrame {
        if idx >= self.route_slots.get() {
            crate::invariant();
        }
        /* SAFETY: `RouteTable` owns the initialized `frames` column; `idx` is
        inside `route_slots`, and callers only hold table-local borrows while
        walking or mutating one route-frame list. */
        unsafe { &*self.frames_ptr().add(idx) }
    }

    #[inline]
    fn frame_ptr_checked(&self, idx: usize) -> *mut RouteFrame {
        if idx >= self.route_slots.get() {
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
    fn record_frame_entry(&self, idx: usize, role_slots: usize, role_from: u8, arm: u8) -> u16 {
        /* SAFETY: `idx` is a route frame owned by this table. The entry field
        is initialized with the frame, and this method performs the single
        mutation for a route decision before waking lane waiters. */
        let entry = unsafe { &mut (*self.frame_ptr_checked(idx)).entry };
        let mut generation = entry.generation.wrapping_add(1);
        if generation == 0 {
            generation = 1;
        }
        entry.generation = generation;
        entry.arm = arm;
        entry.seen_mask = 0;
        if (role_from as usize) < role_slots {
            entry.seen_mask |= Self::seen_bit(role_from as usize);
        }
        generation
    }

    #[inline]
    fn mark_unseen_role(&self, idx: usize, role_bit: u16) -> Option<u8> {
        /* SAFETY: `idx` is inside the table-owned initialized frame column.
        The mutable entry borrow is local to this method and records exactly one
        role observation for this route generation. */
        let entry = unsafe { &mut (*self.frame_ptr_checked(idx)).entry };
        if entry.generation == 0 || (entry.seen_mask & role_bit) != 0 {
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
        by this `RouteTable`; this is a shared read of one of `lane_slots`
        entries. */
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
    fn pending_hint(&self, lane_idx: usize) -> FrameLabelMask {
        if lane_idx >= self.lane_slots() {
            crate::invariant();
        }
        /* SAFETY: `pending_frame_hint_masks` is initialized with one slot per
        lane during route-table binding or migration. */
        unsafe { *self.pending_frame_hint_masks_ptr().add(lane_idx) }
    }

    #[inline]
    fn set_pending_hint(&self, lane_idx: usize, value: FrameLabelMask) {
        if lane_idx >= self.lane_slots() {
            crate::invariant();
        }
        /* SAFETY: `RouteTable` owns the pending-hint column; `lane_idx` was
        checked against the initialized lane slot count, and this mutation is
        the only live write to that lane's hint slot. */
        unsafe {
            *self.pending_frame_hint_masks_ptr().add(lane_idx) = value;
        }
    }

    #[inline]
    fn waiter_ptr_checked(&self, lane_idx: usize, role_idx: usize) -> *mut WaiterSlot {
        if lane_idx >= self.lane_slots() || role_idx >= MAX_TRACKED_ROLES {
            crate::invariant();
        }
        let slot_idx = lane_idx * MAX_TRACKED_ROLES + role_idx;
        self.waiters_ptr().wrapping_add(slot_idx)
    }

    #[inline]
    fn with_waiter_mut<R>(
        &self,
        lane_idx: usize,
        role_idx: usize,
        f: impl FnOnce(&mut WaiterSlot) -> R,
    ) -> R {
        /* SAFETY: `waiter_ptr_checked` bounds lane/role inside the initialized
        waiter column. The mutable slot reference is local to this route-table
        callback and never aliases another live waiter borrow. */
        unsafe { f(&mut *self.waiter_ptr_checked(lane_idx, role_idx)) }
    }

    #[inline]
    fn slot_for_scope(&self, lane_idx: usize, coord: ScopeCoord) -> Option<usize> {
        let mut current = self.lane_head(lane_idx);
        while current != Self::FRAME_LIST_END {
            let idx = current as usize;
            if self.frame_ref(idx).scope == coord.scope {
                return Some(idx);
            }
            current = self.frame_ref(idx).next;
        }
        None
    }

    fn slot_or_alloc(&self, lane_idx: usize, coord: ScopeCoord) -> Option<usize> {
        if let Some(idx) = Self::slot_for_scope(self, lane_idx, coord) {
            return Some(idx);
        }
        if self.route_slots.get() == 0 {
            return None;
        }
        let idx = self.pop_free_slot()?;
        let head = self.lane_head(lane_idx);
        self.with_frame_mut(idx, |frame| *frame = RouteFrame::assign(coord, head));
        self.set_lane_head(lane_idx, idx as u16);
        Some(idx)
    }

    fn reclaim_completed_route_slot(&self, lane_idx: usize, slot_idx: usize, role_count: u8) {
        let role_mask = Self::complete_seen_mask(Self::role_slot_count(role_count));
        if role_mask == 0 {
            return;
        }
        let frame = self.frame_ref(slot_idx);
        if frame.entry.generation == 0 || (frame.entry.seen_mask & role_mask) != role_mask {
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
    }

    #[inline]
    fn seen_bit(role_idx: usize) -> u16 {
        if role_idx >= u16::BITS as usize {
            crate::invariant();
        }
        1u16 << (role_idx as u32)
    }

    pub(crate) fn record_with_role_count(
        &self,
        lane: Lane,
        role_count: u8,
        role_from: u8,
        scope: ScopeId,
        arm: u8,
    ) -> u16 {
        let coord = crate::invariant_some(ScopeCoord::from_scope(scope));
        let lane_idx = self.lane_slot(lane);
        let slot_idx = match Self::slot_or_alloc(self, lane_idx, coord) {
            Some(slot_idx) => slot_idx,
            None => {
                crate::invariant();
            }
        };
        let role_slots = Self::role_slot_count(role_count);
        let generation = self.record_frame_entry(slot_idx, role_slots, role_from, arm);

        let mut role_idx = 0usize;
        while role_idx < MAX_TRACKED_ROLES {
            let waker = self.with_waiter_mut(lane_idx, role_idx, WaiterSlot::take);
            if let Some(waker) = waker {
                waker.wake();
            }
            role_idx += 1;
        }
        generation
    }

    pub(crate) fn poll_with_role_count(
        &self,
        lane: Lane,
        role_count: u8,
        role: u8,
        scope: ScopeId,
        cx: &mut Context<'_>,
    ) -> Poll<u8> {
        let role_slots = Self::role_slot_count(role_count);
        if (role as usize) >= role_slots {
            return Poll::Ready(0);
        }
        let coord = crate::invariant_some(ScopeCoord::from_scope(scope));
        let lane_idx = self.lane_slot(lane);
        let slot_idx = match Self::slot_or_alloc(self, lane_idx, coord) {
            Some(idx) => idx,
            None => return Poll::Pending,
        };
        let role_bit = Self::seen_bit(role as usize);
        let arm = self.mark_unseen_role(slot_idx, role_bit);
        if let Some(arm) = arm {
            self.reclaim_completed_route_slot(lane_idx, slot_idx, role_count);
            return Poll::Ready(arm);
        }

        self.with_waiter_mut(lane_idx, role as usize, |waiter| waiter.set(cx.waker()));
        Poll::Pending
    }

    pub(crate) fn peek_with_role_count(
        &self,
        lane: Lane,
        role_count: u8,
        role: u8,
        scope: ScopeId,
    ) -> Option<u8> {
        let role_slots = Self::role_slot_count(role_count);
        if (role as usize) >= role_slots {
            return None;
        }
        let coord = ScopeCoord::from_scope(scope)?;
        let lane_idx = self.lane_slot(lane);
        let slot_idx = Self::slot_for_scope(self, lane_idx, coord)?;
        let entry = self.frame_ref(slot_idx).entry;
        let role_bit = Self::seen_bit(role as usize);
        (entry.generation != 0 && (entry.seen_mask & role_bit) == 0).then_some(entry.arm)
    }

    pub(crate) fn has_pending_lane_with_role_count(
        &self,
        role_count: u8,
        role: u8,
        scope: ScopeId,
        lane: Lane,
    ) -> bool {
        let role_slots = Self::role_slot_count(role_count);
        if (role as usize) >= role_slots {
            return false;
        }
        let coord = match ScopeCoord::from_scope(scope) {
            Some(coord) => coord,
            None => return false,
        };
        let role_bit = Self::seen_bit(role as usize);
        let lane_idx = self.lane_slot(lane);
        if let Some(slot_idx) = Self::slot_for_scope(self, lane_idx, coord) {
            let entry = self.frame_ref(slot_idx).entry;
            return entry.generation != 0 && (entry.seen_mask & role_bit) == 0;
        }
        false
    }

    #[inline]
    pub(crate) fn pending_frame_hint_mask_for_lane(&self, lane: Lane) -> FrameLabelMask {
        if self.route_slots.get() == 0 {
            return FrameLabelMask::EMPTY;
        }
        let lane_idx = self.lane_slot(lane);
        self.pending_hint(lane_idx)
    }

    pub(crate) fn update_pending_frame_hint_mask_for_lane(
        &self,
        lane: Lane,
        before: FrameLabelMask,
        after: FrameLabelMask,
    ) {
        if before == after || self.route_slots.get() == 0 {
            return;
        }
        let lane_idx = self.lane_slot(lane);
        self.set_pending_hint(lane_idx, after);
    }

    pub(crate) fn has_pending_frame_hint_for_lane(
        &self,
        lane: Lane,
        frame_label_mask: FrameLabelMask,
    ) -> bool {
        if self.route_slots.get() == 0 {
            return false;
        }
        let lane_idx = self.lane_slot(lane);
        self.pending_hint(lane_idx).intersects(frame_label_mask)
    }

    pub(crate) fn reset_lane(&self, lane: Lane) {
        if self.route_slots.get() == 0 {
            return;
        }
        let lane_idx = self.lane_slot(lane);
        let mut current = self.lane_head(lane_idx);
        self.set_lane_head(lane_idx, Self::FRAME_LIST_END);
        while current != Self::FRAME_LIST_END {
            let idx = current as usize;
            let next = self.frame_ref(idx).next;
            self.push_free_slot(idx);
            current = next;
        }
        self.set_pending_hint(lane_idx, FrameLabelMask::EMPTY);
        let mut role_idx = 0usize;
        while role_idx < MAX_TRACKED_ROLES {
            self.with_waiter_mut(lane_idx, role_idx, |waiter| waiter.clear());
            role_idx += 1;
        }
    }

    pub(crate) fn wake_lane_waiters(&self, lane: Lane) {
        if self.route_slots.get() == 0 {
            return;
        }
        let lane_idx = self.lane_slot(lane);
        let mut role_idx = 0usize;
        while role_idx < MAX_TRACKED_ROLES {
            let waker = self.with_waiter_mut(lane_idx, role_idx, WaiterSlot::take);
            if let Some(waker) = waker {
                waker.wake();
            }
            role_idx += 1;
        }
    }
}
