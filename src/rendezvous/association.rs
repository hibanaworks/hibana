//! Association table for mapping session IDs to lanes.
//!
//! Maintains bidirectional mapping between session IDs and lanes,
//! plus active/inactive status tracking.
//!
//! # Unsafe Owner Contract
//!
//! This module owns the session/lane association storage. Unsafe blocks here may
//! access backing arrays only through the table's lane capacity and must keep
//! sid-to-lane and lane-to-sid entries synchronized before waking waiters.

use core::{cell::UnsafeCell, marker::PhantomData, task::Waker};

use crate::session::types::{Lane, SessionId};

use super::waiter::WaiterSlot;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SessionFaultKind {
    TransportClosed,
    PeerReset,
    DecodeFailed,
    ProtocolViolation,
    EndpointDropped,
    ProgressInvariantViolated,
}

impl SessionFaultKind {
    const ABSENT_CODE: u8 = 0;

    #[inline]
    const fn encode(self) -> u8 {
        match self {
            Self::TransportClosed => 1,
            Self::PeerReset => 2,
            Self::DecodeFailed => 3,
            Self::ProtocolViolation => 4,
            Self::EndpointDropped => 5,
            Self::ProgressInvariantViolated => 6,
        }
    }

    #[inline]
    const fn decode(raw: u8) -> Option<Self> {
        match raw {
            Self::ABSENT_CODE => None,
            1 => Some(Self::TransportClosed),
            2 => Some(Self::PeerReset),
            3 => Some(Self::DecodeFailed),
            4 => Some(Self::ProtocolViolation),
            5 => Some(Self::EndpointDropped),
            6 => Some(Self::ProgressInvariantViolated),
            _ => crate::invariant(),
        }
    }
}

#[derive(Clone, Copy)]
struct AssocStorageParts {
    lane_to_sid: *mut SessionId,
    ref_counts: *mut u8,
    faults: *mut u8,
    waiters: *mut WaiterSlot,
}

/// Association table (session ID ↔ lane mapping).
///
/// Tracks which lane is assigned to each lane slot inside the configured
/// rendezvous lane range and whether it is active.
pub(super) struct AssocTable {
    lane_base: u32,
    lane_slots: u16,
    lane_to_sid: UnsafeCell<*mut SessionId>,
    ref_counts: UnsafeCell<*mut u8>,
    faults: UnsafeCell<*mut u8>,
    waiters: UnsafeCell<*mut WaiterSlot>,
    _no_send_sync: PhantomData<*mut ()>,
}

impl AssocTable {
    pub(super) unsafe fn init_empty(dst: *mut Self) {
        /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
        unsafe {
            core::ptr::addr_of_mut!((*dst).lane_base).write(0);
            core::ptr::addr_of_mut!((*dst).lane_slots).write(0);
            core::ptr::addr_of_mut!((*dst).lane_to_sid)
                .write(UnsafeCell::new(core::ptr::null_mut()));
            core::ptr::addr_of_mut!((*dst).ref_counts)
                .write(UnsafeCell::new(core::ptr::null_mut()));
            core::ptr::addr_of_mut!((*dst).faults).write(UnsafeCell::new(core::ptr::null_mut()));
            core::ptr::addr_of_mut!((*dst).waiters).write(UnsafeCell::new(core::ptr::null_mut()));
            core::ptr::addr_of_mut!((*dst)._no_send_sync).write(PhantomData);
        }
    }

    #[inline]
    pub(super) const fn storage_align() -> usize {
        let sid_align = core::mem::align_of::<SessionId>();
        let count_align = core::mem::align_of::<u8>();
        let waiter_align = core::mem::align_of::<WaiterSlot>();
        let sid_count_align = if sid_align > count_align {
            sid_align
        } else {
            count_align
        };
        if sid_count_align > waiter_align {
            sid_count_align
        } else {
            waiter_align
        }
    }

    #[inline]
    const fn align_up(value: usize, align: usize) -> usize {
        if align == 0 {
            crate::invariant();
        }
        let mask = align - 1;
        if value > usize::MAX - mask {
            crate::invariant();
        }
        (value + mask) & !mask
    }

    #[inline]
    const fn checked_add_usize(lhs: usize, rhs: usize) -> usize {
        if lhs > usize::MAX - rhs {
            crate::invariant();
        }
        lhs + rhs
    }

    #[inline]
    const fn checked_mul_usize(lhs: usize, rhs: usize) -> usize {
        if lhs != 0 && rhs > usize::MAX / lhs {
            crate::invariant();
        }
        lhs * rhs
    }

    #[inline]
    const fn checked_sub_usize(lhs: usize, rhs: usize) -> usize {
        if lhs < rhs {
            crate::invariant();
        }
        lhs - rhs
    }

    #[inline]
    pub(super) const fn storage_bytes(lane_slots: usize) -> usize {
        let sid_bytes = Self::checked_mul_usize(lane_slots, core::mem::size_of::<SessionId>());
        let count_offset = Self::align_up(sid_bytes, core::mem::align_of::<u8>());
        let count_bytes = Self::checked_mul_usize(lane_slots, core::mem::size_of::<u8>());
        let fault_offset = Self::align_up(
            Self::checked_add_usize(count_offset, count_bytes),
            core::mem::align_of::<u8>(),
        );
        let fault_bytes = Self::checked_mul_usize(lane_slots, core::mem::size_of::<u8>());
        let waiter_offset = Self::align_up(
            Self::checked_add_usize(fault_offset, fault_bytes),
            core::mem::align_of::<WaiterSlot>(),
        );
        Self::checked_add_usize(
            waiter_offset,
            Self::checked_mul_usize(lane_slots, core::mem::size_of::<WaiterSlot>()),
        )
    }

    unsafe fn storage_parts(storage: *mut u8, lane_slots: usize) -> AssocStorageParts {
        let lane_to_sid = storage.cast::<SessionId>();
        let count_offset = Self::checked_sub_usize(
            Self::align_up(
                Self::checked_add_usize(
                    storage as usize,
                    Self::checked_mul_usize(lane_slots, core::mem::size_of::<SessionId>()),
                ),
                core::mem::align_of::<u8>(),
            ),
            storage as usize,
        );
        let ref_counts = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { storage.add(count_offset) }.cast::<u8>();
        let fault_offset = Self::checked_sub_usize(
            Self::align_up(
                Self::checked_add_usize(
                    Self::checked_add_usize(storage as usize, count_offset),
                    Self::checked_mul_usize(lane_slots, core::mem::size_of::<u8>()),
                ),
                core::mem::align_of::<u8>(),
            ),
            storage as usize,
        );
        let faults = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { storage.add(fault_offset) }.cast::<u8>();
        let waiter_offset = Self::checked_sub_usize(
            Self::align_up(
                Self::checked_add_usize(
                    Self::checked_add_usize(storage as usize, fault_offset),
                    Self::checked_mul_usize(lane_slots, core::mem::size_of::<u8>()),
                ),
                core::mem::align_of::<WaiterSlot>(),
            ),
            storage as usize,
        );
        let waiters = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { storage.add(waiter_offset) }.cast::<WaiterSlot>();
        AssocStorageParts {
            lane_to_sid,
            ref_counts,
            faults,
            waiters,
        }
    }

    unsafe fn bind_storage(
        &mut self,
        lane_base: u32,
        lane_slots: usize,
        lane_to_sid: *mut SessionId,
        ref_counts: *mut u8,
        faults: *mut u8,
        waiters: *mut WaiterSlot,
    ) {
        let mut idx = 0usize;
        while idx < lane_slots {
            /* SAFETY: the caller supplies exclusive uninitialized storage and this initializer writes all exposed fields before return. */
            unsafe {
                lane_to_sid.add(idx).write(SessionId::new(0));
                ref_counts.add(idx).write(0);
                faults.add(idx).write(SessionFaultKind::ABSENT_CODE);
                WaiterSlot::init_empty(waiters.add(idx));
            }
            idx += 1;
        }
        self.lane_base = lane_base;
        self.lane_slots = lane_slots as u16;
        *self.lane_to_sid.get_mut() = lane_to_sid;
        *self.ref_counts.get_mut() = ref_counts;
        *self.faults.get_mut() = faults;
        *self.waiters.get_mut() = waiters;
    }

    pub(super) unsafe fn bind_from_storage(
        &mut self,
        storage: *mut u8,
        lane_base: u32,
        lane_slots: usize,
    ) {
        let parts = /* SAFETY: the caller supplies the assoc-table arena with `lane_slots` capacity. */ unsafe {
            Self::storage_parts(storage, lane_slots)
        };
        /* SAFETY: the association table owns the lane/session slots and checks slot presence before raw access. */
        unsafe {
            self.bind_storage(
                lane_base,
                lane_slots,
                parts.lane_to_sid,
                parts.ref_counts,
                parts.faults,
                parts.waiters,
            );
        }
    }

    #[inline]
    pub(super) fn is_bound(&self) -> bool {
        !self.lane_to_sid_ptr().is_null()
    }

    pub(super) unsafe fn init_replacement_storage(
        &self,
        storage: *mut u8,
        lane_base: u32,
        lane_slots: usize,
    ) {
        let source_base = self.lane_base;
        let source_slots = self.lane_slots();
        let source_sids = self.lane_to_sid_ptr();
        let source_counts = self.ref_counts_ptr();
        let source_faults = self.faults_ptr();
        let source_waiters = self.waiters_ptr();
        let parts = /* SAFETY: `storage` is the freshly leased assoc-table arena. */ unsafe {
            Self::storage_parts(storage, lane_slots)
        };
        let mut idx = 0usize;
        while idx < lane_slots {
            /* SAFETY: the caller supplies exclusive uninitialized storage and this initializer writes all exposed fields before return. */
            unsafe {
                parts.lane_to_sid.add(idx).write(SessionId::new(0));
                parts.ref_counts.add(idx).write(0);
                parts.faults.add(idx).write(SessionFaultKind::ABSENT_CODE);
                WaiterSlot::init_empty(parts.waiters.add(idx));
            }
            idx += 1;
        }
        let mut source_idx = 0usize;
        while source_idx < source_slots {
            let lane = source_base + source_idx as u32;
            if lane >= lane_base {
                let new_idx = (lane - lane_base) as usize;
                if new_idx < lane_slots {
                    /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
                    unsafe {
                        parts
                            .lane_to_sid
                            .add(new_idx)
                            .write(*source_sids.add(source_idx));
                        parts
                            .ref_counts
                            .add(new_idx)
                            .write(*source_counts.add(source_idx));
                        parts
                            .faults
                            .add(new_idx)
                            .write(*source_faults.add(source_idx));
                        WaiterSlot::init_clone_from(
                            parts.waiters.add(new_idx),
                            &*source_waiters.add(source_idx),
                        );
                    }
                }
            }
            source_idx += 1;
        }
    }

    pub(super) unsafe fn clear_waiters_in_storage(storage: *mut u8, lane_slots: usize) {
        let parts = /* SAFETY: the caller passes an initialized assoc-table arena. */ unsafe {
            Self::storage_parts(storage, lane_slots)
        };
        let mut idx = 0usize;
        while idx < lane_slots {
            /* SAFETY: waiters were initialized as part of assoc-table storage staging or binding. */
            unsafe {
                (*parts.waiters.add(idx)).clear();
            }
            idx += 1;
        }
    }

    pub(super) fn clear_current_waiters(&self) {
        let waiters = self.waiters_ptr();
        let mut idx = 0usize;
        while idx < self.lane_slots() {
            /* SAFETY: waiters_ptr points at the currently bound assoc-table waiter column. */
            unsafe {
                (*waiters.add(idx)).clear();
            }
            idx += 1;
        }
    }

    pub(super) unsafe fn commit_storage(
        &mut self,
        storage: *mut u8,
        lane_base: u32,
        lane_slots: usize,
    ) {
        let parts = /* SAFETY: `storage` is an initialized assoc-table arena staged for publication. */ unsafe {
            Self::storage_parts(storage, lane_slots)
        };
        self.lane_base = lane_base;
        self.lane_slots = lane_slots as u16;
        *self.lane_to_sid.get_mut() = parts.lane_to_sid;
        *self.ref_counts.get_mut() = parts.ref_counts;
        *self.faults.get_mut() = parts.faults;
        *self.waiters.get_mut() = parts.waiters;
    }

    #[inline]
    fn lane_slots(&self) -> usize {
        self.lane_slots as usize
    }

    #[inline]
    fn lane_to_sid_ptr(&self) -> *mut SessionId {
        /* SAFETY: the association table owns the lane/session slots and checks slot presence before raw access. */
        unsafe { *self.lane_to_sid.get() }
    }

    #[inline]
    fn ref_counts_ptr(&self) -> *mut u8 {
        /* SAFETY: the association table owns the lane/session slots and checks slot presence before raw access. */
        unsafe { *self.ref_counts.get() }
    }

    #[inline]
    fn faults_ptr(&self) -> *mut u8 {
        /* SAFETY: the association table owns the lane/session slots and checks slot presence before raw access. */
        unsafe { *self.faults.get() }
    }

    #[inline]
    fn waiters_ptr(&self) -> *mut WaiterSlot {
        /* SAFETY: the association table owns the lane/session slots and checks slot presence before raw access. */
        unsafe { *self.waiters.get() }
    }

    #[inline]
    fn lane_slot(&self, lane: Lane) -> Option<usize> {
        let lane_raw = lane.raw();
        if lane_raw < self.lane_base {
            return None;
        }
        let slot = (lane_raw - self.lane_base) as usize;
        (slot < self.lane_slots()).then_some(slot)
    }

    /// Register a session on an empty lane slot.
    #[inline]
    pub(super) fn register(&self, lane: Lane, sid: SessionId) {
        let Some(idx) = self.lane_slot(lane) else {
            crate::invariant();
        };
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe {
            let sids = self.lane_to_sid_ptr();
            let counts = self.ref_counts_ptr();
            if *counts.add(idx) != 0 {
                crate::invariant();
            }
            sids.add(idx).write(sid);
            counts.add(idx).write(1);
            self.faults_ptr()
                .add(idx)
                .write(SessionFaultKind::ABSENT_CODE);
            (*self.waiters_ptr().add(idx)).clear();
        }
    }

    /// Increment the attachment count for a lane already associated with `sid`.
    ///
    /// Returns the new attachment count on success.
    #[inline]
    pub(super) fn increment(&self, lane: Lane, sid: SessionId) -> Option<u8> {
        let idx = self.lane_slot(lane)?;
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe {
            let sids = self.lane_to_sid_ptr();
            let counts = self.ref_counts_ptr();
            let current = *counts.add(idx);
            if current == 0 || *sids.add(idx) != sid {
                return None;
            }
            if current == u8::MAX {
                return None;
            }
            let next = current + 1;
            counts.add(idx).write(next);
            Some(next)
        }
    }

    /// Decrement the attachment count for `lane` associated with `sid`.
    ///
    /// Returns the remaining attachment count after the decrement, or `None`
    /// if the lane was not associated with `sid`.
    #[inline]
    pub(super) fn decrement(&self, lane: Lane, sid: SessionId) -> Option<u8> {
        let idx = self.lane_slot(lane)?;
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe {
            let sids = self.lane_to_sid_ptr();
            let counts = self.ref_counts_ptr();
            let current = *counts.add(idx);
            if current == 0 || *sids.add(idx) != sid {
                return None;
            }
            let next = current - 1;
            counts.add(idx).write(next);
            if next == 0 {
                sids.add(idx).write(SessionId::new(0));
                self.faults_ptr()
                    .add(idx)
                    .write(SessionFaultKind::ABSENT_CODE);
                (*self.waiters_ptr().add(idx)).clear();
            }
            Some(next)
        }
    }

    #[inline]
    pub(super) fn for_each_lane(&self, sid: SessionId, mut visit: impl FnMut(Lane)) {
        /* SAFETY: the association table owns the lane/session slots and checks slot presence before raw access. */
        unsafe {
            let sids = self.lane_to_sid_ptr();
            let counts = self.ref_counts_ptr();
            let mut idx = 0usize;
            while idx < self.lane_slots() {
                if *counts.add(idx) != 0 && *sids.add(idx) == sid {
                    visit(Lane::new(self.lane_base + idx as u32));
                }
                idx += 1;
            }
        }
    }

    #[inline]
    pub(super) fn register_waiter(&self, sid: SessionId, lane: Lane, waker: &Waker) {
        let Some(idx) = self.lane_slot(lane) else {
            return;
        };
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe {
            let sids = self.lane_to_sid_ptr();
            let counts = self.ref_counts_ptr();
            if *counts.add(idx) == 0 || *sids.add(idx) != sid {
                return;
            }
            (*self.waiters_ptr().add(idx)).set(waker);
        }
    }

    #[inline]
    pub(super) fn clear_waiter(&self, sid: SessionId, lane: Lane) {
        let Some(idx) = self.lane_slot(lane) else {
            return;
        };
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe {
            let sids = self.lane_to_sid_ptr();
            let counts = self.ref_counts_ptr();
            if *counts.add(idx) == 0 || *sids.add(idx) != sid {
                return;
            }
            (*self.waiters_ptr().add(idx)).clear();
        }
    }

    #[inline]
    pub(super) fn wake_session_waiters(&self, sid: SessionId) {
        /* SAFETY: the association table owns the lane/session slots and checks slot presence before raw access. */
        unsafe {
            let sids = self.lane_to_sid_ptr();
            let counts = self.ref_counts_ptr();
            let waiters = self.waiters_ptr();
            let mut idx = 0usize;
            while idx < self.lane_slots() {
                if *counts.add(idx) != 0 && *sids.add(idx) == sid {
                    (*waiters.add(idx)).wake();
                }
                idx += 1;
            }
        }
    }

    /// Get session ID for a lane (if registered).
    #[inline]
    pub(super) fn get_sid(&self, lane: Lane) -> Option<SessionId> {
        let idx = self.lane_slot(lane)?;
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe {
            let counts = self.ref_counts_ptr();
            (*counts.add(idx) != 0).then_some(*self.lane_to_sid_ptr().add(idx))
        }
    }

    #[inline]
    pub(super) fn session_fault(&self, sid: SessionId) -> Option<SessionFaultKind> {
        /* SAFETY: the association table owns the lane/session slots and checks slot presence before raw access. */
        unsafe {
            let sids = self.lane_to_sid_ptr();
            let counts = self.ref_counts_ptr();
            let faults = self.faults_ptr();
            let mut idx = 0usize;
            while idx < self.lane_slots() {
                if *counts.add(idx) != 0 && *sids.add(idx) == sid {
                    let raw = *faults.add(idx);
                    if let Some(kind) = SessionFaultKind::decode(raw) {
                        return Some(kind);
                    }
                }
                idx += 1;
            }
            None
        }
    }

    #[inline]
    pub(super) fn poison_session(
        &self,
        sid: SessionId,
        cause: SessionFaultKind,
    ) -> SessionFaultKind {
        if let Some(existing) = self.session_fault(sid) {
            return existing;
        }
        /* SAFETY: the association table owns the lane/session slots and checks slot presence before raw access. */
        unsafe {
            let sids = self.lane_to_sid_ptr();
            let counts = self.ref_counts_ptr();
            let faults = self.faults_ptr();
            let encoded = cause.encode();
            let mut idx = 0usize;
            while idx < self.lane_slots() {
                if *counts.add(idx) != 0 && *sids.add(idx) == sid {
                    faults.add(idx).write(encoded);
                }
                idx += 1;
            }
        }
        cause
    }
}
