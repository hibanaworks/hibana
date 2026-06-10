use super::{
    Generation, Lane, PhantomData, UnsafeCell, align_up, checked_add_usize, checked_mul_usize,
    checked_sub_usize, lane_storage_align,
};
/// Generation counter table (per-lane).
///
/// Tracks the last seen generation number for each lane to ensure monotonic updates.
pub(crate) struct GenTable {
    lane_base: u32,
    lane_slots: u16,
    pub(crate) lanes: UnsafeCell<*mut u16>,
    present: UnsafeCell<*mut u8>,
    _no_send_sync: PhantomData<*mut ()>,
}

impl Default for GenTable {
    fn default() -> Self {
        Self::empty()
    }
}

impl GenTable {
    pub(crate) const fn empty() -> Self {
        Self {
            lane_base: 0,
            lane_slots: 0,
            lanes: UnsafeCell::new(core::ptr::null_mut()),
            present: UnsafeCell::new(core::ptr::null_mut()),
            _no_send_sync: PhantomData,
        }
    }

    pub(crate) unsafe fn init_empty(dst: *mut Self) {
        /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
        unsafe {
            core::ptr::addr_of_mut!((*dst).lane_base).write(0);
            core::ptr::addr_of_mut!((*dst).lane_slots).write(0);
            core::ptr::addr_of_mut!((*dst).lanes).write(UnsafeCell::new(core::ptr::null_mut()));
            core::ptr::addr_of_mut!((*dst).present).write(UnsafeCell::new(core::ptr::null_mut()));
            core::ptr::addr_of_mut!((*dst)._no_send_sync).write(PhantomData);
        }
    }

    #[inline]
    pub(crate) const fn storage_align() -> usize {
        lane_storage_align()
    }

    #[inline]
    pub(crate) const fn storage_bytes(lane_slots: usize) -> usize {
        let lanes_bytes = checked_mul_usize(lane_slots, core::mem::size_of::<u16>());
        let present_offset = align_up(lanes_bytes, core::mem::align_of::<u8>());
        checked_add_usize(
            present_offset,
            checked_mul_usize(lane_slots, core::mem::size_of::<u8>()),
        )
    }

    pub(crate) unsafe fn bind_storage(
        &mut self,
        lanes: *mut u16,
        present: *mut u8,
        lane_base: u32,
        lane_slots: usize,
    ) {
        let mut idx = 0usize;
        while idx < lane_slots {
            /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
            unsafe {
                lanes.add(idx).write(0);
                present.add(idx).write(0);
            }
            idx += 1;
        }
        self.lane_base = lane_base;
        self.lane_slots = lane_slots as u16;
        *self.lanes.get_mut() = lanes;
        *self.present.get_mut() = present;
    }

    pub(crate) unsafe fn bind_from_storage(
        &mut self,
        storage: *mut u8,
        lane_base: u32,
        lane_slots: usize,
    ) {
        let lanes = storage.cast::<u16>();
        let present_offset = checked_sub_usize(
            align_up(
                checked_add_usize(
                    storage as usize,
                    checked_mul_usize(lane_slots, core::mem::size_of::<u16>()),
                ),
                core::mem::align_of::<u8>(),
            ),
            storage as usize,
        );
        let present = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { storage.add(present_offset) }.cast::<u8>();
        /* SAFETY: the rendezvous table owns initialized slots behind explicit presence state before raw access. */
        unsafe {
            self.bind_storage(lanes, present, lane_base, lane_slots);
        }
    }

    #[inline]
    fn lanes_ptr(&self) -> *mut u16 {
        /* SAFETY: the rendezvous table owns initialized slots behind explicit presence state before raw access. */
        unsafe { *self.lanes.get() }
    }

    #[inline]
    fn present_ptr(&self) -> *mut u8 {
        /* SAFETY: the rendezvous table owns initialized slots behind explicit presence state before raw access. */
        unsafe { *self.present.get() }
    }

    #[inline]
    pub(crate) fn is_bound(&self) -> bool {
        !self.lanes_ptr().is_null()
    }

    #[inline]
    pub(crate) fn storage_ptr(&self) -> *mut u8 {
        self.lanes_ptr().cast::<u8>()
    }

    #[inline]
    pub(crate) const fn storage_bytes_current(&self) -> usize {
        Self::storage_bytes(self.lane_slots as usize)
    }

    pub(crate) unsafe fn rebind_from_storage_preserving(
        &mut self,
        storage: *mut u8,
        lane_base: u32,
        lane_slots: usize,
    ) {
        let old_base = self.lane_base;
        let old_slots = self.lane_slots as usize;
        let old_lanes = self.lanes_ptr();
        let old_present = self.present_ptr();
        let lanes = storage.cast::<u16>();
        let present_offset = checked_sub_usize(
            align_up(
                checked_add_usize(
                    storage as usize,
                    checked_mul_usize(lane_slots, core::mem::size_of::<u16>()),
                ),
                core::mem::align_of::<u8>(),
            ),
            storage as usize,
        );
        let present = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { storage.add(present_offset) }.cast::<u8>();
        let mut idx = 0usize;
        while idx < lane_slots {
            /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
            unsafe {
                lanes.add(idx).write(0);
                present.add(idx).write(0);
            }
            idx += 1;
        }
        let mut old_idx = 0usize;
        while old_idx < old_slots {
            let lane = old_base + old_idx as u32;
            if lane >= lane_base {
                let new_idx = (lane - lane_base) as usize;
                if new_idx < lane_slots {
                    /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
                    unsafe {
                        lanes.add(new_idx).write(*old_lanes.add(old_idx));
                        present.add(new_idx).write(*old_present.add(old_idx));
                    }
                }
            }
            old_idx += 1;
        }
        self.lane_base = lane_base;
        self.lane_slots = lane_slots as u16;
        *self.lanes.get_mut() = lanes;
        *self.present.get_mut() = present;
    }

    #[inline]
    fn lane_slot(&self, lane: Lane) -> Option<usize> {
        let lane_raw = lane.raw();
        if lane_raw < self.lane_base {
            return None;
        }
        let slot = (lane_raw - self.lane_base) as usize;
        (slot < self.lane_slots as usize).then_some(slot)
    }

    #[inline]
    pub(crate) fn publish_prepared(&self, lane: Lane, new: Generation) {
        let idx = self
            .lane_slot(lane)
            .expect("prepared generation publish lane escaped storage");
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe {
            self.lanes_ptr().add(idx).write(new.raw());
            self.present_ptr().add(idx).write(1);
        }
    }

    /// Get last generation for a lane.
    #[inline]
    pub(crate) fn last(&self, lane: Lane) -> Option<Generation> {
        let idx = self.lane_slot(lane)?;
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe {
            (*self.present_ptr().add(idx) != 0)
                .then_some(Generation::new(*self.lanes_ptr().add(idx)))
        }
    }

    /// Reset lane (for release).
    #[inline]
    pub(crate) fn reset_lane(&self, lane: Lane) {
        let Some(idx) = self.lane_slot(lane) else {
            return;
        };
        /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
        unsafe {
            self.lanes_ptr().add(idx).write(0);
            self.present_ptr().add(idx).write(0);
        }
    }
}
