use crate::epf::{verifier::VerifiedImage, vm::Slot};

pub(crate) const SLOT_COUNT: usize = 5;
pub(crate) const CODE_MAX: usize = VerifiedImage::MAX_CODE_LEN;
#[cfg(test)]
pub(crate) const SCRATCH_MAX: usize = 2048;

#[derive(Clone, Copy)]
pub(crate) struct SlotStorage {
    #[cfg(test)]
    active: [u8; CODE_MAX],
    staging: [u8; CODE_MAX],
    #[cfg(test)]
    backup: [u8; CODE_MAX],
    #[cfg(test)]
    scratch: [u8; SCRATCH_MAX],
}

impl SlotStorage {
    pub(crate) const fn new() -> Self {
        Self {
            #[cfg(test)]
            active: [0; CODE_MAX],
            staging: [0; CODE_MAX],
            #[cfg(test)]
            backup: [0; CODE_MAX],
            #[cfg(test)]
            scratch: [0; SCRATCH_MAX],
        }
    }

    #[inline]
    #[cfg(test)]
    pub(crate) fn staging(&self) -> &[u8] {
        &self.staging
    }

    #[inline]
    pub(crate) fn staging_mut(&mut self) -> &mut [u8] {
        &mut self.staging
    }

    #[cfg(test)]
    pub(crate) fn copy_active_to_staging(&mut self, len: usize) {
        self.staging[..len].copy_from_slice(&self.active[..len]);
    }

    #[cfg(test)]
    pub(crate) fn copy_active_to_backup(&mut self, len: usize) {
        self.backup[..len].copy_from_slice(&self.active[..len]);
    }

    #[cfg(test)]
    pub(crate) fn copy_backup_to_active(&mut self, len: usize) {
        self.active[..len].copy_from_slice(&self.backup[..len]);
    }

    #[cfg(test)]
    pub(crate) fn copy_staging_to_active(&mut self, len: usize) {
        self.active[..len].copy_from_slice(&self.staging[..len]);
    }

    #[inline]
    #[cfg(test)]
    pub(crate) fn active_and_scratch_mut(&mut self) -> (&mut [u8], &mut [u8]) {
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

pub(crate) struct SlotArena {
    slots: [SlotStorage; SLOT_COUNT],
}

impl SlotArena {
    pub(crate) const fn new() -> Self {
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

    #[cfg(test)]
    pub(crate) fn storage(&self, slot: Slot) -> &SlotStorage {
        &self.slots[slot_index(slot)]
    }

    pub(crate) fn storage_mut(&mut self, slot: Slot) -> &mut SlotStorage {
        &mut self.slots[slot_index(slot)]
    }
}

impl Default for SlotArena {
    fn default() -> Self {
        Self::new()
    }
}

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
