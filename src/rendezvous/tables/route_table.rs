use super::{
    Context, FrameLabelMask, Lane, MAX_TRACKED_ROLES, PhantomData, Poll, ScopeId, ScopeKind,
    UnsafeCell, WaiterSlot,
};
// # Unsafe Owner Contract
//
// This fragment owns route-decision table frames and route-scope head columns.
// Unsafe operations bind resident storage once, keep route frames in explicit
// free lists, and access table slots only after scope canonicalization and
// table-capacity checks performed by this owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ScopeCoord {
    canonical: ScopeId,
}

impl ScopeCoord {
    fn from_scope(scope: ScopeId) -> Option<Self> {
        if scope.is_none() || scope.kind() != ScopeKind::Route {
            return None;
        }
        Some(Self {
            canonical: scope.canonical(),
        })
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
            scope: coord.canonical,
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
    reclaim_delta: usize,
}

pub(crate) struct RouteTable {
    frames: UnsafeCell<*mut RouteFrame>,
    route_slots: usize,
    lane_base: u32,
    lane_slots: u16,
    lane_heads: UnsafeCell<*mut u16>,
    free_head: UnsafeCell<*mut u16>,
    pending_frame_hint_masks: UnsafeCell<*mut FrameLabelMask>,
    change_generation: UnsafeCell<u16>,
    waiters: UnsafeCell<*mut WaiterSlot>,
    _no_send_sync: PhantomData<*mut ()>,
}

mod storage;

impl RouteTable {
    #[inline]
    fn lane_slot(&self, lane: Lane) -> usize {
        if lane.raw() < self.lane_base {
            crate::invariant();
        }
        let lane_idx = (lane.raw() - self.lane_base) as usize;
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
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe { &*self.frames_ptr().add(idx) }
    }

    #[inline]
    fn frame_ptr_at(&self, idx: usize) -> *mut RouteFrame {
        self.frames_ptr().wrapping_add(idx)
    }

    #[inline]
    fn slot_for_scope(&self, lane_idx: usize, coord: ScopeCoord) -> Option<usize> {
        let mut current = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { *self.lane_heads_ptr().add(lane_idx) };
        while current != Self::FRAME_LIST_END {
            let idx = current as usize;
            if self.frame_ref(idx).scope == coord.canonical {
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
        if self.route_slots == 0 {
            return None;
        }
        let idx = self.pop_free_slot()?;
        let head = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { *self.lane_heads_ptr().add(lane_idx) };
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe {
            *self.frame_ptr_at(idx) = RouteFrame::assign(coord, head);
        }
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe {
            *self.lane_heads_ptr().add(lane_idx) = idx as u16;
        }
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
        let mut current = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { *self.lane_heads_ptr().add(lane_idx) };
        while current != Self::FRAME_LIST_END {
            let current_idx = current as usize;
            let next = self.frame_ref(current_idx).next;
            if current_idx == slot_idx {
                if prev == Self::FRAME_LIST_END {
                    /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
                    unsafe {
                        *self.lane_heads_ptr().add(lane_idx) = next;
                    }
                } else {
                    /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
                    unsafe {
                        (*self.frame_ptr_at(prev as usize)).next = next;
                    }
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

    #[inline]
    fn bump_change_generation(&self) {
        let generation = /* SAFETY: the pointer comes from pinned owner storage and this path holds the unique mutable access for the borrow. */ unsafe { &mut *self.change_generation.get() };
        let next = generation.wrapping_add(1);
        *generation = if next == 0 { 1 } else { next };
    }

    #[inline]
    pub(crate) fn change_generation(&self) -> u16 {
        /* SAFETY: the rendezvous table owns initialized slots behind explicit presence state before raw access. */
        unsafe { *self.change_generation.get() }
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
        /* SAFETY: the slot index comes from owned table allocation and this operation owns the mutation. */
        let entry = unsafe { &mut (*self.frame_ptr_at(slot_idx)).entry };
        let mut generation = entry.generation.wrapping_add(1);
        if generation == 0 {
            generation = 1;
        }
        entry.generation = generation;
        entry.arm = arm;
        entry.seen_mask = 0;
        let role_slots = Self::role_slot_count(role_count);
        if (role_from as usize) < role_slots {
            entry.seen_mask |= Self::seen_bit(role_from as usize);
        }
        self.bump_change_generation();

        let waiters = self.waiters_ptr();
        let mut role_idx = 0usize;
        while role_idx < MAX_TRACKED_ROLES {
            /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
            unsafe {
                (*waiters.add(lane_idx * MAX_TRACKED_ROLES + role_idx)).wake();
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
        /* SAFETY: the slot index comes from owned table allocation and this operation owns the mutation. */
        let entry = unsafe { &mut (*self.frame_ptr_at(slot_idx)).entry };
        let role_bit = Self::seen_bit(role as usize);
        if entry.generation != 0 && (entry.seen_mask & role_bit) == 0 {
            entry.seen_mask |= role_bit;
            let arm = entry.arm;
            self.reclaim_completed_route_slot(lane_idx, slot_idx, role_count);
            self.bump_change_generation();
            return Poll::Ready(arm);
        }

        let waiters = self.waiters_ptr();
        let slot = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { &mut *waiters.add(lane_idx * MAX_TRACKED_ROLES + role as usize) };
        slot.set(cx.waker());
        Poll::Pending
    }

    pub(crate) fn acknowledge_with_role_count(
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
        /* SAFETY: the slot index comes from owned table allocation and this operation owns the mutation. */
        let entry = unsafe { &mut (*self.frame_ptr_at(slot_idx)).entry };
        if entry.generation == 0 {
            return None;
        }
        let role_bit = Self::seen_bit(role as usize);
        if (entry.seen_mask & role_bit) != 0 {
            return None;
        }
        entry.seen_mask |= role_bit;
        let arm = entry.arm;
        self.reclaim_completed_route_slot(lane_idx, slot_idx, role_count);
        self.bump_change_generation();
        Some(arm)
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
        if self.route_slots == 0 {
            return FrameLabelMask::EMPTY;
        }
        let lane_idx = self.lane_slot(lane);
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe { *self.pending_frame_hint_masks_ptr().add(lane_idx) }
    }

    pub(crate) fn update_pending_frame_hint_mask_for_lane(
        &self,
        lane: Lane,
        before: FrameLabelMask,
        after: FrameLabelMask,
    ) {
        if before == after || self.route_slots == 0 {
            return;
        }
        let lane_idx = self.lane_slot(lane);
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe {
            *self.pending_frame_hint_masks_ptr().add(lane_idx) = after;
        }
        self.bump_change_generation();
    }

    pub(crate) fn has_pending_frame_hint_for_lane(
        &self,
        lane: Lane,
        frame_label_mask: FrameLabelMask,
    ) -> bool {
        if self.route_slots == 0 {
            return false;
        }
        let lane_idx = self.lane_slot(lane);
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe { *self.pending_frame_hint_masks_ptr().add(lane_idx) }.intersects(frame_label_mask)
    }

    pub(crate) fn reset_lane(&self, lane: Lane) {
        if self.route_slots == 0 {
            return;
        }
        let lane_idx = self.lane_slot(lane);
        let mut current = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { *self.lane_heads_ptr().add(lane_idx) };
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe {
            *self.lane_heads_ptr().add(lane_idx) = Self::FRAME_LIST_END;
        }
        while current != Self::FRAME_LIST_END {
            let idx = current as usize;
            let next = self.frame_ref(idx).next;
            self.push_free_slot(idx);
            current = next;
        }
        let pending_frame_hint_masks = self.pending_frame_hint_masks_ptr();
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe {
            *pending_frame_hint_masks.add(lane_idx) = FrameLabelMask::EMPTY;
        }
        let waiters = self.waiters_ptr();
        let mut role_idx = 0usize;
        while role_idx < MAX_TRACKED_ROLES {
            /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
            unsafe {
                (*waiters.add(lane_idx * MAX_TRACKED_ROLES + role_idx)).clear();
            }
            role_idx += 1;
        }
        self.bump_change_generation();
    }

    pub(crate) fn wake_lane_waiters(&self, lane: Lane) {
        if self.route_slots == 0 {
            return;
        }
        let lane_idx = self.lane_slot(lane);
        let waiters = self.waiters_ptr();
        let mut role_idx = 0usize;
        while role_idx < MAX_TRACKED_ROLES {
            /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
            unsafe {
                (*waiters.add(lane_idx * MAX_TRACKED_ROLES + role_idx)).wake();
            }
            role_idx += 1;
        }
    }
}
