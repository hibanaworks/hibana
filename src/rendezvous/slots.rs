use crate::epf::{
    verifier::{Header, VerifiedImage},
    vm::Slot,
};

pub const SLOT_COUNT: usize = 5;
pub const CODE_MAX: usize = VerifiedImage::MAX_CODE_LEN;
pub const SCRATCH_MAX: usize = Header::max_mem_len();

const fn ensure_index(idx: usize) -> usize {
    if idx >= SLOT_COUNT {
        panic!("slot index out of range");
    }
    idx
}

#[derive(Clone, Copy)]
pub struct SlotStorage {
    active: [u8; CODE_MAX],
    staging: [u8; CODE_MAX],
    backup: [u8; CODE_MAX],
    scratch: [u8; SCRATCH_MAX],
}

impl SlotStorage {
    pub const fn new() -> Self {
        Self {
            active: [0; CODE_MAX],
            staging: [0; CODE_MAX],
            backup: [0; CODE_MAX],
            scratch: [0; SCRATCH_MAX],
        }
    }

    #[inline]
    pub fn active(&self) -> &[u8] {
        &self.active
    }

    #[inline]
    pub fn active_mut(&mut self) -> &mut [u8] {
        &mut self.active
    }

    #[inline]
    pub fn staging(&self) -> &[u8] {
        &self.staging
    }

    #[inline]
    pub fn staging_mut(&mut self) -> &mut [u8] {
        &mut self.staging
    }

    #[inline]
    pub fn backup(&self) -> &[u8] {
        &self.backup
    }

    #[inline]
    pub fn backup_mut(&mut self) -> &mut [u8] {
        &mut self.backup
    }

    #[inline]
    pub fn scratch_mut(&mut self) -> &mut [u8] {
        &mut self.scratch
    }

    pub fn copy_active_to_staging(&mut self, len: usize) {
        self.staging[..len].copy_from_slice(&self.active[..len]);
    }

    pub fn copy_active_to_backup(&mut self, len: usize) {
        self.backup[..len].copy_from_slice(&self.active[..len]);
    }

    pub fn copy_backup_to_active(&mut self, len: usize) {
        self.active[..len].copy_from_slice(&self.backup[..len]);
    }

    pub fn copy_staging_to_active(&mut self, len: usize) {
        self.active[..len].copy_from_slice(&self.staging[..len]);
    }

    #[inline]
    pub fn active_and_scratch_mut(&mut self) -> (&mut [u8], &mut [u8]) {
        let active = &mut self.active;
        let scratch = &mut self.scratch;
        (active, scratch)
    }
}

impl Default for SlotStorage {
    fn default() -> Self {
        Self::new()
    }
}

pub struct SlotArena {
    slots: [SlotStorage; SLOT_COUNT],
}

impl SlotArena {
    pub const fn new() -> Self {
        Self {
            slots: [
                SlotStorage::new(),
                SlotStorage::new(),
                SlotStorage::new(),
                SlotStorage::new(),
                SlotStorage::new(),
            ],
        }
    }

    pub fn lease<const IDX: usize>(&mut self) -> SlotLease<'_, IDX> {
        let idx = ensure_index(IDX);
        SlotLease {
            storage: &mut self.slots[idx],
        }
    }

    pub fn lease_dynamic(&mut self, slot: Slot) -> SlotLeaseDyn<'_> {
        let idx = slot_index(slot);
        SlotLeaseDyn {
            storage: &mut self.slots[idx],
        }
    }

    pub fn storage(&self, slot: Slot) -> &SlotStorage {
        &self.slots[slot_index(slot)]
    }

    pub fn storage_mut(&mut self, slot: Slot) -> &mut SlotStorage {
        &mut self.slots[slot_index(slot)]
    }
}

impl Default for SlotArena {
    fn default() -> Self {
        Self::new()
    }
}

pub struct SlotLease<'arena, const IDX: usize> {
    storage: &'arena mut SlotStorage,
}

impl<'arena, const IDX: usize> SlotLease<'arena, IDX> {
    #[inline]
    pub fn storage(&self) -> &SlotStorage {
        self.storage
    }

    #[inline]
    pub fn storage_mut(&mut self) -> &mut SlotStorage {
        self.storage
    }
}

pub struct SlotLeaseDyn<'arena> {
    storage: &'arena mut SlotStorage,
}

impl<'arena> SlotLeaseDyn<'arena> {
    #[inline]
    pub fn storage(&self) -> &SlotStorage {
        self.storage
    }

    #[inline]
    pub fn storage_mut(&mut self) -> &mut SlotStorage {
        self.storage
    }
}

#[inline]
pub const fn slot_index(slot: Slot) -> usize {
    match slot {
        Slot::Forward => 0,
        Slot::EndpointRx => 1,
        Slot::EndpointTx => 2,
        Slot::Rendezvous => 3,
        Slot::Route => 4,
    }
}
