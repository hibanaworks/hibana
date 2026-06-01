use super::{ArrayMap, CONTROL_PLAN_SLOTS, EffIndex, Lane, PhantomData, PolicyMode, UnsafeCell};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PolicyKey {
    pub(crate) lane: Lane,
    pub(crate) eff_index: EffIndex,
    tag: u8,
}

pub(crate) struct PolicyEntries {
    policies: ArrayMap<PolicyKey, PolicyMode, CONTROL_PLAN_SLOTS>,
}

impl PolicyEntries {
    pub(crate) unsafe fn init_empty(dst: *mut Self) {
        /* SAFETY: the caller supplies exclusive uninitialized storage and this initializer writes all exposed fields before return. */
        unsafe {
            ArrayMap::init_empty(core::ptr::addr_of_mut!((*dst).policies));
        }
    }

    fn register(
        &mut self,
        lane: Lane,
        eff_index: EffIndex,
        tag: u8,
        policy: PolicyMode,
    ) -> Result<(), PolicyMode> {
        let key = PolicyKey {
            lane,
            eff_index,
            tag,
        };
        self.policies.insert(key, policy)
    }

    pub(crate) fn get(&self, lane: Lane, eff_index: EffIndex, tag: u8) -> Option<PolicyMode> {
        let key = PolicyKey {
            lane,
            eff_index,
            tag,
        };
        self.policies.get(&key).copied()
    }

    fn reset_lane(&mut self, lane: Lane) {
        self.policies.retain(|key, _policy| key.lane != lane);
    }
}

pub(crate) struct PolicyTable {
    lane_base: u32,
    lane_slots: u16,
    entries: UnsafeCell<*mut PolicyEntries>,
    _no_send_sync: PhantomData<*mut ()>,
}

impl Default for PolicyTable {
    fn default() -> Self {
        Self::empty()
    }
}

impl PolicyTable {
    pub(crate) const fn empty() -> Self {
        Self {
            lane_base: 0,
            lane_slots: 0,
            entries: UnsafeCell::new(core::ptr::null_mut()),
            _no_send_sync: PhantomData,
        }
    }

    pub(crate) unsafe fn init_empty(dst: *mut Self) {
        /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
        unsafe {
            core::ptr::addr_of_mut!((*dst).lane_base).write(0);
            core::ptr::addr_of_mut!((*dst).lane_slots).write(0);
            core::ptr::addr_of_mut!((*dst).entries).write(UnsafeCell::new(core::ptr::null_mut()));
            core::ptr::addr_of_mut!((*dst)._no_send_sync).write(PhantomData);
        }
    }

    #[inline]
    pub(crate) const fn storage_align() -> usize {
        core::mem::align_of::<PolicyEntries>()
    }

    #[inline]
    pub(crate) const fn storage_bytes(lane_slots: usize) -> usize {
        if lane_slots == 0 {
            0
        } else {
            core::mem::size_of::<PolicyEntries>()
        }
    }

    pub(crate) unsafe fn bind_storage(
        &mut self,
        entries: *mut PolicyEntries,
        lane_base: u32,
        lane_slots: usize,
    ) {
        /* SAFETY: the caller supplies exclusive uninitialized storage and this initializer writes all exposed fields before return. */
        unsafe {
            PolicyEntries::init_empty(entries);
        }
        self.lane_base = lane_base;
        self.lane_slots = lane_slots as u16;
        *self.entries.get_mut() = entries;
    }

    pub(crate) unsafe fn bind_from_storage(
        &mut self,
        storage: *mut u8,
        lane_base: u32,
        lane_slots: usize,
    ) {
        /* SAFETY: the rendezvous table owns initialized slots behind explicit presence state before raw access. */
        unsafe {
            self.bind_storage(storage.cast::<PolicyEntries>(), lane_base, lane_slots);
        }
    }

    #[inline]
    pub(crate) fn entries_ptr(&self) -> *mut PolicyEntries {
        /* SAFETY: the rendezvous table owns initialized slots behind explicit presence state before raw access. */
        unsafe { *self.entries.get() }
    }

    #[inline]
    pub(crate) fn is_bound(&self) -> bool {
        !self.entries_ptr().is_null()
    }

    #[inline]
    pub(crate) fn rebind_lane_span(&mut self, lane_base: u32, lane_slots: usize) {
        self.lane_base = lane_base;
        self.lane_slots = lane_slots as u16;
    }

    #[inline]
    #[cfg(test)]
    pub(crate) fn storage_ptr(&self) -> *mut u8 {
        self.entries_ptr().cast::<u8>()
    }

    #[inline]
    #[cfg(test)]
    pub(crate) const fn storage_bytes_current(&self) -> usize {
        Self::storage_bytes(self.lane_slots as usize)
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

    pub(crate) fn register(
        &self,
        lane: Lane,
        eff_index: EffIndex,
        tag: u8,
        policy: PolicyMode,
    ) -> Result<(), PolicyMode> {
        if policy.is_static() {
            return Ok(());
        }
        if self.lane_slot(lane).is_none() || !self.is_bound() {
            return Err(policy);
        }
        /* SAFETY: the pointer comes from pinned owner storage and this path holds the unique mutable access for the borrow. */
        unsafe { (&mut *self.entries_ptr()).register(lane, eff_index, tag, policy) }
    }

    pub(crate) fn get(&self, lane: Lane, eff_index: EffIndex, tag: u8) -> Option<PolicyMode> {
        self.lane_slot(lane)?;
        if !self.is_bound() {
            return None;
        }
        /* SAFETY: the pointer comes from pinned owner storage and this path only creates a shared borrow. */
        unsafe { (&*self.entries_ptr()).get(lane, eff_index, tag) }
    }

    pub(crate) fn reset_lane(&self, lane: Lane) {
        if self.lane_slot(lane).is_none() || !self.is_bound() {
            return;
        }
        /* SAFETY: the pointer comes from pinned owner storage and this path holds unique mutable access for the borrow. */
        unsafe {
            (&mut *self.entries_ptr()).reset_lane(lane);
        }
    }
}
