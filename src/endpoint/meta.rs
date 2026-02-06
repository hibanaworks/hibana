//! Helpers for resolving label/flag metadata from typestate without relying on transport meta.

use crate::global::{RoleProgram, role_program::LocalStepMeta, typestate::PhaseCursor};

/// Resolve the current step metadata using the typestate cursor and role program.
///
/// This is intended for metadata-less transports: endpoints can reconstruct the
/// expected label/peer/control bits from typestate and ScopeTrace instead of
/// relying on transport-provided LogicalFrameMeta.
pub fn current_step_meta<'prog, const ROLE: u8, LocalSteps, Mint>(
    cursor: &PhaseCursor<ROLE>,
    program: &RoleProgram<'prog, ROLE, LocalSteps, Mint>,
) -> Option<LocalStepMeta>
where
    Mint: crate::control::cap::MintConfigMarker,
{
    let eff = cursor.eff_index()?;
    program.step_meta_for(eff)
}
