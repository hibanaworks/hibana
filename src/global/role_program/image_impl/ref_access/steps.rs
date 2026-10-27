//! Field reads from the immutable role image after its constructor has checked
//! every event and referenced fact. Raw lane readers remain the construction
//! and mutation-test oracle; a runtime row is an ordinal in this sealed image.

use super::{LocalDependency, PackedEventConflict, RoleImageRef, invalid_resident_descriptor};
use crate::{
    eff::EffIndex,
    global::{
        const_dsl::ScopeId,
        role_program::{
            PackedLocalEventRow, ROLE_IMAGE_CONFLICT_STRIDE, ROLE_IMAGE_DEPENDENCY_STRIDE,
            ROLE_IMAGE_EVENT_STRIDE, RoleLocalDirection,
        },
        typestate::{PackedLocalDependency, RouteChoiceMark},
    },
};

#[cfg(all(test, hibana_repo_tests))]
mod tests;

impl RoleImageRef {
    #[inline(always)]
    const fn step_offset(&self, index: usize) -> usize {
        if index >= self.local_step_count() {
            invalid_resident_descriptor();
        }
        self.columns.events.offset as usize + index * ROLE_IMAGE_EVENT_STRIDE
    }

    #[inline(always)]
    const fn step_u16(&self, offset: usize) -> u16 {
        u16::from_le_bytes([self.blob.byte_at(offset), self.blob.byte_at(offset + 1)])
    }

    #[inline(always)]
    pub(crate) const fn local_step_effect(&self, index: usize) -> EffIndex {
        EffIndex::from_dense_ordinal(self.step_u16(self.step_offset(index)) as usize)
    }

    #[inline(always)]
    pub(crate) const fn local_step_direction(&self, index: usize) -> RoleLocalDirection {
        RoleLocalDirection::from_validated_flags(self.blob.byte_at(self.step_offset(index) + 8))
    }

    #[inline(always)]
    pub(crate) const fn local_step_logical_role(&self, index: usize) -> u8 {
        let atom = self
            .program()
            .event_atom_at(self.local_step_effect(index).dense_ordinal());
        if matches!(self.local_step_direction(index), RoleLocalDirection::Recv) {
            atom.to
        } else {
            atom.from
        }
    }

    #[inline(always)]
    pub(crate) const fn local_step_scope(&self, index: usize) -> ScopeId {
        match ScopeId::decode_raw(self.step_u16(self.step_offset(index) + 6)) {
            Some(scope) => scope,
            None => invalid_resident_descriptor(),
        }
    }

    #[inline(always)]
    pub(crate) const fn local_step_choice(&self, index: usize) -> RouteChoiceMark {
        if self.blob.byte_at(self.step_offset(index) + 8)
            & PackedLocalEventRow::FLAG_CHOICE_DETERMINANT
            != 0
        {
            RouteChoiceMark::Determinant
        } else {
            RouteChoiceMark::Ordinary
        }
    }

    #[inline(always)]
    pub(crate) const fn local_step_lane(&self, index: usize) -> Option<u8> {
        if index >= self.local_step_count() {
            return None;
        }
        Some(
            self.blob
                .byte_at(self.columns.lanes.offset as usize + index),
        )
    }

    #[inline(always)]
    pub(crate) const fn event_conflict_for_index(&self, index: usize) -> PackedEventConflict {
        let row = self.step_u16(self.step_offset(index) + 4);
        if row == u16::MAX {
            return PackedEventConflict::none();
        }
        let offset =
            self.columns.conflicts.offset as usize + row as usize * ROLE_IMAGE_CONFLICT_STRIDE;
        PackedEventConflict::from_validated_raw(self.step_u16(offset))
    }

    #[inline(always)]
    pub(crate) const fn dependency_for_index(&self, index: usize) -> Option<LocalDependency> {
        let row = self.step_u16(self.step_offset(index) + 2);
        if row == u16::MAX {
            return None;
        }
        let offset =
            self.columns.dependencies.offset as usize + row as usize * ROLE_IMAGE_DEPENDENCY_STRIDE;
        PackedLocalDependency::from_packed_parts(
            self.step_u16(offset),
            self.step_u16(offset + 2),
            self.step_u16(offset + 4),
            self.step_u16(offset + 6),
        )
        .validated_dependency()
    }
}
