//! Association table for mapping session IDs to lanes.
//!
//! Maintains bidirectional mapping between session IDs and lanes,
//! plus active/inactive status tracking.

use core::{cell::UnsafeCell, marker::PhantomData, task::Waker};

use crate::control::types::{Lane, SessionId};

use super::waiter::WaiterSlot;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SessionFaultKind {
    DeadlineExceeded,
    TransportClosed,
    PeerReset,
    DecodeFailed,
    ProtocolViolation,
    EndpointDropped,
    GenerationMismatch,
    ProgressInvariantViolated,
}

impl SessionFaultKind {
    const NONE: u8 = 0;

    #[inline]
    const fn encode(self) -> u8 {
        match self {
            Self::DeadlineExceeded => 1,
            Self::TransportClosed => 2,
            Self::PeerReset => 3,
            Self::DecodeFailed => 4,
            Self::ProtocolViolation => 5,
            Self::EndpointDropped => 6,
            Self::GenerationMismatch => 7,
            Self::ProgressInvariantViolated => 8,
        }
    }

    #[inline]
    const fn decode(raw: u8) -> Option<Self> {
        match raw {
            1 => Some(Self::DeadlineExceeded),
            2 => Some(Self::TransportClosed),
            3 => Some(Self::PeerReset),
            4 => Some(Self::DecodeFailed),
            5 => Some(Self::ProtocolViolation),
            6 => Some(Self::EndpointDropped),
            7 => Some(Self::GenerationMismatch),
            8 => Some(Self::ProgressInvariantViolated),
            _ => None,
        }
    }
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

impl Default for AssocTable {
    fn default() -> Self {
        Self::empty()
    }
}

impl AssocTable {
    pub(super) const fn empty() -> Self {
        Self {
            lane_base: 0,
            lane_slots: 0,
            lane_to_sid: UnsafeCell::new(core::ptr::null_mut()),
            ref_counts: UnsafeCell::new(core::ptr::null_mut()),
            faults: UnsafeCell::new(core::ptr::null_mut()),
            waiters: UnsafeCell::new(core::ptr::null_mut()),
            _no_send_sync: PhantomData,
        }
    }

    pub(super) unsafe fn init_empty(dst: *mut Self) {
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
        let mask = align.saturating_sub(1);
        (value + mask) & !mask
    }

    #[inline]
    pub(super) const fn storage_bytes(lane_slots: usize) -> usize {
        let sid_bytes = lane_slots.saturating_mul(core::mem::size_of::<SessionId>());
        let count_offset = Self::align_up(sid_bytes, core::mem::align_of::<u8>());
        let count_bytes = lane_slots.saturating_mul(core::mem::size_of::<u8>());
        let fault_offset = Self::align_up(
            count_offset.saturating_add(count_bytes),
            core::mem::align_of::<u8>(),
        );
        let fault_bytes = lane_slots.saturating_mul(core::mem::size_of::<u8>());
        let waiter_offset = Self::align_up(
            fault_offset.saturating_add(fault_bytes),
            core::mem::align_of::<WaiterSlot>(),
        );
        waiter_offset.saturating_add(lane_slots.saturating_mul(core::mem::size_of::<WaiterSlot>()))
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
            unsafe {
                lane_to_sid.add(idx).write(SessionId::new(0));
                ref_counts.add(idx).write(0);
                faults.add(idx).write(SessionFaultKind::NONE);
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
        let lane_to_sid = storage.cast::<SessionId>();
        let count_offset = Self::align_up(
            storage as usize + lane_slots.saturating_mul(core::mem::size_of::<SessionId>()),
            core::mem::align_of::<u8>(),
        ) - storage as usize;
        let ref_counts = unsafe { storage.add(count_offset) }.cast::<u8>();
        let fault_offset = Self::align_up(
            storage as usize + count_offset + lane_slots.saturating_mul(core::mem::size_of::<u8>()),
            core::mem::align_of::<u8>(),
        ) - storage as usize;
        let faults = unsafe { storage.add(fault_offset) }.cast::<u8>();
        let waiter_offset = Self::align_up(
            storage as usize + fault_offset + lane_slots.saturating_mul(core::mem::size_of::<u8>()),
            core::mem::align_of::<WaiterSlot>(),
        ) - storage as usize;
        let waiters = unsafe { storage.add(waiter_offset) }.cast::<WaiterSlot>();
        unsafe {
            self.bind_storage(
                lane_base,
                lane_slots,
                lane_to_sid,
                ref_counts,
                faults,
                waiters,
            );
        }
    }

    #[inline]
    pub(super) fn is_bound(&self) -> bool {
        !self.lane_to_sid_ptr().is_null()
    }

    #[inline]
    pub(super) fn storage_ptr(&self) -> *mut u8 {
        self.lane_to_sid_ptr().cast::<u8>()
    }

    #[inline]
    pub(super) const fn storage_bytes_current(&self) -> usize {
        Self::storage_bytes(self.lane_slots as usize)
    }

    pub(super) unsafe fn rebind_from_storage_preserving(
        &mut self,
        storage: *mut u8,
        lane_base: u32,
        lane_slots: usize,
    ) {
        let old_base = self.lane_base;
        let old_slots = self.lane_slots();
        let old_sids = self.lane_to_sid_ptr();
        let old_counts = self.ref_counts_ptr();
        let old_faults = self.faults_ptr();
        let old_waiters = self.waiters_ptr();
        let lane_to_sid = storage.cast::<SessionId>();
        let count_offset = Self::align_up(
            storage as usize + lane_slots.saturating_mul(core::mem::size_of::<SessionId>()),
            core::mem::align_of::<u8>(),
        ) - storage as usize;
        let ref_counts = unsafe { storage.add(count_offset) }.cast::<u8>();
        let fault_offset = Self::align_up(
            storage as usize + count_offset + lane_slots.saturating_mul(core::mem::size_of::<u8>()),
            core::mem::align_of::<u8>(),
        ) - storage as usize;
        let faults = unsafe { storage.add(fault_offset) }.cast::<u8>();
        let waiter_offset = Self::align_up(
            storage as usize + fault_offset + lane_slots.saturating_mul(core::mem::size_of::<u8>()),
            core::mem::align_of::<WaiterSlot>(),
        ) - storage as usize;
        let waiters = unsafe { storage.add(waiter_offset) }.cast::<WaiterSlot>();
        let mut idx = 0usize;
        while idx < lane_slots {
            unsafe {
                lane_to_sid.add(idx).write(SessionId::new(0));
                ref_counts.add(idx).write(0);
                faults.add(idx).write(SessionFaultKind::NONE);
                WaiterSlot::init_empty(waiters.add(idx));
            }
            idx += 1;
        }
        let mut old_idx = 0usize;
        while old_idx < old_slots {
            let lane = old_base + old_idx as u32;
            if lane >= lane_base {
                let new_idx = (lane - lane_base) as usize;
                if new_idx < lane_slots {
                    unsafe {
                        lane_to_sid.add(new_idx).write(*old_sids.add(old_idx));
                        ref_counts.add(new_idx).write(*old_counts.add(old_idx));
                        faults.add(new_idx).write(*old_faults.add(old_idx));
                        waiters
                            .add(new_idx)
                            .write(core::ptr::read(old_waiters.add(old_idx)));
                    }
                }
            }
            old_idx += 1;
        }
        self.lane_base = lane_base;
        self.lane_slots = lane_slots as u16;
        *self.lane_to_sid.get_mut() = lane_to_sid;
        *self.ref_counts.get_mut() = ref_counts;
        *self.faults.get_mut() = faults;
        *self.waiters.get_mut() = waiters;
    }

    #[inline]
    fn lane_slots(&self) -> usize {
        self.lane_slots as usize
    }

    #[inline]
    fn lane_to_sid_ptr(&self) -> *mut SessionId {
        unsafe { *self.lane_to_sid.get() }
    }

    #[inline]
    fn ref_counts_ptr(&self) -> *mut u8 {
        unsafe { *self.ref_counts.get() }
    }

    #[inline]
    fn faults_ptr(&self) -> *mut u8 {
        unsafe { *self.faults.get() }
    }

    #[inline]
    fn waiters_ptr(&self) -> *mut WaiterSlot {
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

    /// Register a session on a lane that is currently unused.
    #[inline]
    pub(super) fn register(&self, lane: Lane, sid: SessionId) {
        let Some(idx) = self.lane_slot(lane) else {
            debug_assert!(false, "register called for lane outside rendezvous range");
            return;
        };
        unsafe {
            let sids = self.lane_to_sid_ptr();
            let counts = self.ref_counts_ptr();
            debug_assert!(
                *counts.add(idx) == 0,
                "register called on lane with active attachments"
            );
            sids.add(idx).write(sid);
            counts.add(idx).write(1);
            self.faults_ptr().add(idx).write(SessionFaultKind::NONE);
            (*self.waiters_ptr().add(idx)).clear();
        }
    }

    /// Increment the attachment count for a lane already associated with `sid`.
    ///
    /// Returns the new attachment count on success.
    #[inline]
    pub(super) fn increment(&self, lane: Lane, sid: SessionId) -> Option<u8> {
        let idx = self.lane_slot(lane)?;
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
                self.faults_ptr().add(idx).write(SessionFaultKind::NONE);
                (*self.waiters_ptr().add(idx)).clear();
            }
            Some(next)
        }
    }

    /// Find lane for a session ID.
    #[inline]
    pub(super) fn find_lane(&self, sid: SessionId) -> Option<Lane> {
        unsafe {
            let sids = self.lane_to_sid_ptr();
            let counts = self.ref_counts_ptr();
            let mut idx = 0usize;
            while idx < self.lane_slots() {
                if *counts.add(idx) != 0 && *sids.add(idx) == sid {
                    return Some(Lane::new(self.lane_base + idx as u32));
                }
                idx += 1;
            }
            None
        }
    }

    #[inline]
    pub(super) fn for_each_lane(&self, sid: SessionId, mut visit: impl FnMut(Lane)) {
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

    /// Check if a lane is active.
    #[inline]
    pub(super) fn is_active(&self, lane: Lane) -> bool {
        let Some(idx) = self.lane_slot(lane) else {
            return false;
        };
        unsafe { *self.ref_counts_ptr().add(idx) > 0 }
    }

    /// Get session ID for a lane (if registered).
    #[inline]
    pub(super) fn get_sid(&self, lane: Lane) -> Option<SessionId> {
        let idx = self.lane_slot(lane)?;
        unsafe {
            let counts = self.ref_counts_ptr();
            (*counts.add(idx) != 0).then_some(*self.lane_to_sid_ptr().add(idx))
        }
    }

    #[inline]
    pub(super) fn session_fault(&self, sid: SessionId) -> Option<SessionFaultKind> {
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
