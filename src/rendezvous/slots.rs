use core::marker::PhantomData;
#[cfg(test)]
use crate::epf::vm::Slot;
#[cfg(test)]
use core::ptr::NonNull;
#[cfg(test)]
use std::boxed::Box;

#[cfg(test)]
pub(crate) const SLOT_COUNT: usize = 5;
#[cfg(test)]
pub(crate) const CODE_MAX: usize = 2048;
#[cfg(test)]
pub(crate) const SCRATCH_MAX: usize = 2048;

#[derive(Clone, Copy)]
pub(crate) struct SlotStorage {
    #[cfg(test)]
    staging_ptr: *mut u8,
    #[cfg(test)]
    staging_len: usize,
    #[cfg(test)]
    active_ptr: *mut u8,
    #[cfg(test)]
    active_len: usize,
    #[cfg(test)]
    backup_ptr: *mut u8,
    #[cfg(test)]
    backup_len: usize,
    #[cfg(test)]
    scratch_ptr: *mut u8,
    #[cfg(test)]
    scratch_len: usize,
    #[cfg(not(test))]
    _storage: PhantomData<*mut ()>,
}

impl SlotStorage {
    #[inline]
    const fn empty() -> Self {
        #[cfg(test)]
        {
            Self {
                staging_ptr: core::ptr::null_mut(),
                staging_len: 0,
                active_ptr: core::ptr::null_mut(),
                active_len: 0,
                backup_ptr: core::ptr::null_mut(),
                backup_len: 0,
                scratch_ptr: core::ptr::null_mut(),
                scratch_len: 0,
            }
        }
        #[cfg(not(test))]
        {
            Self {
                _storage: PhantomData,
            }
        }
    }

    #[cfg(test)]
    pub(crate) unsafe fn init_empty(dst: *mut Self) {
        unsafe {
            core::ptr::addr_of_mut!((*dst).staging_ptr).write(core::ptr::null_mut());
            core::ptr::addr_of_mut!((*dst).staging_len).write(0);
            core::ptr::addr_of_mut!((*dst).active_ptr).write(core::ptr::null_mut());
            core::ptr::addr_of_mut!((*dst).active_len).write(0);
            core::ptr::addr_of_mut!((*dst).backup_ptr).write(core::ptr::null_mut());
            core::ptr::addr_of_mut!((*dst).backup_len).write(0);
            core::ptr::addr_of_mut!((*dst).scratch_ptr).write(core::ptr::null_mut());
            core::ptr::addr_of_mut!((*dst).scratch_len).write(0);
        }
    }

    #[inline]
    #[cfg(test)]
    fn empty_slice_ptr() -> *mut u8 {
        NonNull::<u8>::dangling().as_ptr()
    }

    #[inline]
    #[cfg(test)]
    pub(crate) fn bind_staging(&mut self, staging: &mut [u8]) {
        self.staging_ptr = staging.as_mut_ptr();
        self.staging_len = staging.len();
    }

    #[cfg(test)]
    #[inline]
    pub(crate) fn bind_test_storage(
        &mut self,
        active: &mut [u8],
        staging: &mut [u8],
        backup: &mut [u8],
        scratch: &mut [u8],
    ) {
        self.active_ptr = active.as_mut_ptr();
        self.active_len = active.len();
        self.bind_staging(staging);
        self.backup_ptr = backup.as_mut_ptr();
        self.backup_len = backup.len();
        self.scratch_ptr = scratch.as_mut_ptr();
        self.scratch_len = scratch.len();
    }

    #[cfg(test)]
    pub(crate) fn new() -> Self {
        let mut storage = Self::empty();
        let active = std::vec![0u8; CODE_MAX].into_boxed_slice();
        let staging = std::vec![0u8; CODE_MAX].into_boxed_slice();
        let backup = std::vec![0u8; CODE_MAX].into_boxed_slice();
        let scratch = std::vec![0u8; SCRATCH_MAX].into_boxed_slice();
        let active = Box::leak(active);
        let staging = Box::leak(staging);
        let backup = Box::leak(backup);
        let scratch = Box::leak(scratch);
        storage.bind_test_storage(active, staging, backup, scratch);
        storage
    }

    #[inline]
    #[cfg(test)]
    pub(crate) fn staging(&self) -> &[u8] {
        let ptr = if self.staging_ptr.is_null() {
            Self::empty_slice_ptr()
        } else {
            self.staging_ptr
        };
        unsafe { core::slice::from_raw_parts(ptr.cast_const(), self.staging_len) }
    }

    #[inline]
    #[cfg(test)]
    pub(crate) fn staging_mut(&mut self) -> &mut [u8] {
        let ptr = if self.staging_ptr.is_null() {
            Self::empty_slice_ptr()
        } else {
            self.staging_ptr
        };
        unsafe { core::slice::from_raw_parts_mut(ptr, self.staging_len) }
    }

    #[cfg(test)]
    pub(crate) fn copy_active_to_staging(&mut self, len: usize) {
        let active =
            unsafe { core::slice::from_raw_parts(self.active_ptr.cast_const(), self.active_len) };
        self.staging_mut()[..len].copy_from_slice(&active[..len]);
    }

    #[cfg(test)]
    pub(crate) fn copy_active_to_backup(&mut self, len: usize) {
        let active =
            unsafe { core::slice::from_raw_parts(self.active_ptr.cast_const(), self.active_len) };
        let backup = unsafe { core::slice::from_raw_parts_mut(self.backup_ptr, self.backup_len) };
        backup[..len].copy_from_slice(&active[..len]);
    }

    #[cfg(test)]
    pub(crate) fn copy_backup_to_active(&mut self, len: usize) {
        let backup =
            unsafe { core::slice::from_raw_parts(self.backup_ptr.cast_const(), self.backup_len) };
        let active = unsafe { core::slice::from_raw_parts_mut(self.active_ptr, self.active_len) };
        active[..len].copy_from_slice(&backup[..len]);
    }

    #[cfg(test)]
    pub(crate) fn copy_staging_to_active(&mut self, len: usize) {
        let staging =
            unsafe { core::slice::from_raw_parts(self.staging_ptr.cast_const(), self.staging_len) };
        let active = unsafe { core::slice::from_raw_parts_mut(self.active_ptr, self.active_len) };
        active[..len].copy_from_slice(&staging[..len]);
    }

    #[inline]
    #[cfg(test)]
    pub(crate) fn active_and_scratch_mut(&mut self) -> (&mut [u8], &mut [u8]) {
        let active = unsafe { core::slice::from_raw_parts_mut(self.active_ptr, self.active_len) };
        let scratch =
            unsafe { core::slice::from_raw_parts_mut(self.scratch_ptr, self.scratch_len) };
        (active, scratch)
    }
}

impl Default for SlotStorage {
    fn default() -> Self {
        Self::empty()
    }
}

pub(crate) struct SlotArena {
    #[cfg(test)]
    slots: *mut SlotStorage,
    #[cfg(test)]
    slot_count: usize,
    #[cfg(not(test))]
    storage: SlotStorage,
    _no_send_sync: PhantomData<*mut ()>,
}

impl SlotArena {
    pub(crate) const fn empty() -> Self {
        #[cfg(test)]
        {
            Self {
                slots: core::ptr::null_mut(),
                slot_count: 0,
                _no_send_sync: PhantomData,
            }
        }
        #[cfg(not(test))]
        {
            Self {
                storage: SlotStorage::empty(),
                _no_send_sync: PhantomData,
            }
        }
    }

    pub(crate) unsafe fn init_empty(dst: *mut Self) {
        unsafe {
            #[cfg(test)]
            {
                core::ptr::addr_of_mut!((*dst).slots).write(core::ptr::null_mut());
                core::ptr::addr_of_mut!((*dst).slot_count).write(0);
            }
            #[cfg(not(test))]
            {
                core::ptr::addr_of_mut!((*dst).storage).write(SlotStorage::empty());
            }
            core::ptr::addr_of_mut!((*dst)._no_send_sync).write(PhantomData);
        }
    }

    #[inline]
    #[cfg(test)]
    pub(crate) const fn storage_align() -> usize {
        core::mem::align_of::<SlotStorage>()
    }

    #[inline]
    #[cfg(test)]
    pub(crate) const fn storage_bytes() -> usize {
        SLOT_COUNT.saturating_mul(core::mem::size_of::<SlotStorage>())
    }

    #[inline]
    #[cfg(test)]
    pub(crate) fn slots_ptr(&self) -> *mut SlotStorage {
        self.slots
    }

    #[cfg(test)]
    unsafe fn bind_storage(&mut self, slots: *mut SlotStorage, slot_count: usize) {
        let mut idx = 0usize;
        while idx < slot_count {
            unsafe {
                SlotStorage::init_empty(slots.add(idx));
            }
            idx += 1;
        }
        self.slots = slots;
        self.slot_count = slot_count;
    }

    #[cfg(test)]
    pub(crate) unsafe fn bind_from_storage(&mut self, storage: *mut u8) {
        unsafe {
            self.bind_storage(storage.cast::<SlotStorage>(), SLOT_COUNT);
        }
    }

    #[cfg(test)]
    pub(crate) fn storage(&self, slot: Slot) -> &SlotStorage {
        assert!(
            !self.slots_ptr().is_null() && self.slot_count == SLOT_COUNT,
            "slot arena storage must be bound"
        );
        unsafe { &*self.slots_ptr().add(slot_index(slot)) }
    }

    #[cfg(test)]
    pub(crate) fn storage_mut(&mut self, slot: Slot) -> &mut SlotStorage {
        assert!(
            !self.slots_ptr().is_null() && self.slot_count == SLOT_COUNT,
            "slot arena storage must be bound"
        );
        unsafe { &mut *self.slots_ptr().add(slot_index(slot)) }
    }

    #[cfg(test)]
    pub(crate) fn new() -> Self {
        let mut arena = Self::empty();
        let mut slots = std::vec::Vec::with_capacity(SLOT_COUNT);
        let mut idx = 0usize;
        while idx < SLOT_COUNT {
            slots.push(SlotStorage::new());
            idx += 1;
        }
        let ptr = Box::leak(slots.into_boxed_slice()).as_mut_ptr();
        arena.slots = ptr;
        arena.slot_count = SLOT_COUNT;
        arena
    }
}

impl Default for SlotArena {
    fn default() -> Self {
        Self::empty()
    }
}

#[cfg(test)]
#[inline]
pub(crate) const fn slot_index(slot: Slot) -> usize {
    match slot {
        Slot::Forward => 0,
        Slot::EndpointRx => 1,
        Slot::EndpointTx => 2,
        Slot::Rendezvous => 3,
        Slot::Route => 4,
    }
}
