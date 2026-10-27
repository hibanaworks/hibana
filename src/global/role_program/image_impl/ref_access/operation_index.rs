use super::{RoleImageRef, invalid_resident_descriptor};

impl RoleImageRef {
    const fn operation_key_for_step(&self, step: usize) -> u32 {
        let atom = self
            .program()
            .event_atom_at(self.local_step_effect(step).dense_ordinal());
        self.local_step_direction(step)
            .operation_index_key(atom.from, atom.to, atom.label)
    }

    const fn operation_step_at(&self, slot: usize) -> usize {
        match self.lanes().operation_index_step(slot) {
            Some(step) if step < self.local_step_count() => step,
            Some(_) | None => invalid_resident_descriptor(),
        }
    }

    /// A strictly sorted permutation of all row ordinals is a complete index.
    /// Duplicate, omitted, out-of-range and misordered entries are rejected
    /// before the immutable image can be used by an Endpoint.
    pub(super) const fn validate_operation_index(&self) {
        let mut slot = 0;
        let mut previous_key = 0;
        let mut previous_step = 0;
        while slot < self.local_step_count() {
            let step = self.operation_step_at(slot);
            let key = self.operation_key_for_step(step);
            if slot != 0 && (key < previous_key || (key == previous_key && step <= previous_step)) {
                invalid_resident_descriptor();
            }
            previous_key = key;
            previous_step = step;
            slot += 1;
        }
    }

    fn operation_bound(&self, key: u32, upper: bool) -> usize {
        let mut low = 0;
        let mut high = self.local_step_count();
        while low < high {
            let middle = low + (high - low) / 2;
            let candidate = self.operation_key_for_step(self.operation_step_at(middle));
            if candidate < key || (upper && candidate == key) {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        low
    }

    /// This narrows immutable message identity only. Every candidate still
    /// undergoes live ownership, branch, dependency and repetition admission.
    pub(crate) fn receive_candidates(
        &self,
        role: u8,
        label: u8,
    ) -> impl Iterator<Item = usize> + '_ {
        let key = (u32::from(role) << 8) | u32::from(label);
        let range = self.operation_bound(key, false)..self.operation_bound(key, true);
        range.map(|slot| self.operation_step_at(slot))
    }

    /// All outbound rows with this logical sender and label, including completed
    /// rows. Live admission separately checks payload, ownership and progress.
    pub(crate) fn outbound_candidates(
        &self,
        role: u8,
        label: u8,
    ) -> impl Iterator<Item = usize> + Clone + '_ {
        let key = (1 << 16) | (u32::from(role) << 8) | u32::from(label);
        let range = self.operation_bound(key, false)..self.operation_bound(key, true);
        range.map(|slot| self.operation_step_at(slot))
    }
}
