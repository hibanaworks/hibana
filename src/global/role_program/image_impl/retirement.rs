//! Source-only knowledge of a permanently departed roll entry resource.
//! These temporary facts build/check immutable image rows, never live owners.
use crate::global::{compiled::images::CompiledProgramRef, const_dsl::ScopeId};

pub(super) const fn actor_in_scope(
    program: &CompiledProgramRef,
    scope: ScopeId,
    mut reference: u16,
) -> Option<u8> {
    let Some((first, _, finish)) = program.source_scope_bounds(scope) else {
        return None;
    };
    let mut remaining = program.actor_decision_count();
    while reference >= 256 {
        if remaining == 0 {
            return None;
        }
        remaining -= 1;
        let Some((choice, [left, right])) = program.actor_decision_at(reference as usize - 256)
        else {
            return None;
        };
        let Some((start, split, end)) = program.source_scope_bounds(choice) else {
            return None;
        };
        reference = if start <= first && finish <= split {
            left
        } else if split <= first && finish <= end {
            right
        } else {
            return None;
        };
    }
    Some(reference as u8)
}

/// A completed receipt in `known[actor]` proves departure from the entry owner.
/// Every edge uses a mandatory, nonrepeating source message and the same actor
/// on its receive/send boundary. Lexical order across par is not a causal edge.
pub(super) const fn knowledge(
    program: &CompiledProgramRef,
    scope: ScopeId,
    transfer: usize,
) -> Option<[u16; 256]> {
    if !program.roll_precedes(scope, transfer)
        || !program.source_mandatory_with_scope(scope, transfer)
    {
        return None;
    }
    let Some(resource) = program.event_endpoint(transfer) else {
        return None;
    };
    let Some((first, _, finish)) = program.source_scope_bounds(scope) else {
        return None;
    };
    let mut event = first;
    let mut entry = None;
    while event < finish {
        let (from, to) = program.event_roles(event);
        let side = if from == resource
            || matches!(program.event_endpoint(event), Some(role) if role == resource)
        {
            0
        } else if to == resource {
            1
        } else {
            event += 1;
            continue;
        };
        let Some(actors) = program.actor_binding_at(event) else {
            return None;
        };
        entry = actor_in_scope(program, scope, actors[side]);
        break;
    }
    let Some(entry) = entry else {
        return None;
    };
    let Some([_, receiver]) = program.actor_binding_at(transfer) else {
        return None;
    };
    let Some(receiver) = actor_in_scope(program, scope, receiver) else {
        return None;
    };
    if receiver == entry {
        return None;
    }

    // A once-only source predecessor is already past. Every other possible
    // return, including an earlier repeatable occurrence, keeps reentry open.
    event = 0;
    while event < program.columns.atom_count() {
        if matches!(program.event_endpoint(event), Some(role) if role == resource)
            && !(program.source_nonrepeating(event) && program.source_precedes(event, first))
        {
            let Some([_, target]) = program.actor_binding_at(event) else {
                return None;
            };
            match actor_in_scope(program, scope, target) {
                Some(target) if target != entry => (),
                Some(_) | None => return None,
            }
        }
        event += 1;
    }

    let mut known = [u16::MAX; 256];
    known[receiver as usize] = transfer as u16;
    event = transfer + 1;
    while event < program.columns.atom_count() {
        if program.source_mandatory_with_scope(scope, event) {
            let Some([sender, receiver]) = program.actor_binding_at(event) else {
                return None;
            };
            if let (Some(sender), Some(receiver)) = (
                actor_in_scope(program, scope, sender),
                actor_in_scope(program, scope, receiver),
            ) {
                let before = known[sender as usize];
                if before != u16::MAX
                    && known[receiver as usize] == u16::MAX
                    && program.source_precedes(before as usize, event)
                {
                    known[receiver as usize] = event as u16;
                }
            }
        }
        event += 1;
    }
    Some(known)
}
