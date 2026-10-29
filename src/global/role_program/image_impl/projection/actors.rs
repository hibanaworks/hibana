//! Compile actor-local event membership from the checked global image.
//!
//! This is a construction-time stream, not operation admission. A row with a
//! conditional actor remains guarded by the original canonical route context.
//! Parallel order and activation belong to the local dependency/control data.

use super::super::event_rows::RoleLocalDirection;
use crate::global::compiled::images::CompiledProgramRef;

#[cfg(kani)]
mod kani;
#[cfg(all(test, hibana_repo_tests))]
mod tests;

pub(in crate::global::role_program::image_impl) struct ActorEvent {
    pub(in crate::global::role_program::image_impl) event: usize,
    pub(in crate::global::role_program::image_impl) direction: RoleLocalDirection,
}

/// One bit per shared actor decision, supplied by the compiler/constructor.
/// Bits mean that an actor is a possible leaf, never that it owns an endpoint
/// in the current visit. No mutable holder state survives construction.
pub(in crate::global::role_program::image_impl) struct ActorEventProjection<'a> {
    program: &'a CompiledProgramRef,
    actor: u8,
    membership: &'a [u8],
    event: usize,
    side: u8,
}

const fn reference_contains(reference: u16, actor: u8, membership: &[u8]) -> bool {
    if reference < 256 {
        reference == actor as u16
    } else {
        let decision = reference as usize - 256;
        if decision / 8 >= membership.len() {
            crate::invariant();
        }
        membership[decision / 8] & (1 << (decision % 8)) != 0
    }
}

impl<'a> ActorEventProjection<'a> {
    pub(in crate::global::role_program::image_impl) const fn new(
        program: &'a CompiledProgramRef,
        actor: u8,
        membership: &'a mut [u8],
    ) -> Option<Self> {
        let decisions = program.actor_decision_count();
        let bytes = decisions.div_ceil(8);
        if membership.len() < bytes {
            return None;
        }
        let mut index = 0;
        while index < bytes {
            membership[index] = 0;
            index += 1;
        }
        index = 0;
        while index < decisions {
            let Some((_, [left, right])) = program.actor_decision_at(index) else {
                crate::invariant();
            };
            // Checked actor references point strictly backwards. Each decision
            // is visited once, even when many events share the same subgraph.
            if reference_contains(left, actor, membership)
                || reference_contains(right, actor, membership)
            {
                membership[index / 8] |= 1 << (index % 8);
            }
            index += 1;
        }
        Some(Self {
            program,
            actor,
            membership,
            event: 0,
            side: 0,
        })
    }

    pub(in crate::global::role_program::image_impl) const fn next(&mut self) -> Option<ActorEvent> {
        while self.event < self.program.columns.atom_count() {
            let event = self.event;
            let (from, to) = self.program.event_roles(event);
            if !self.program.has_actor_bindings() {
                self.event += 1;
                if let Some(direction) =
                    super::super::event_rows::role_local_direction(self.actor, from, to)
                {
                    return Some(ActorEvent { event, direction });
                }
                continue;
            }
            // Ordinary programs bind logical roles to themselves; an owned
            // image carries the compiled binding for every event.
            let [sender, receiver] = match self.program.actor_binding_at(event) {
                Some(binding) => binding,
                None => [from as u16, to as u16],
            };
            let (reference, direction) = if self.side == 0 {
                if from == to {
                    if sender != receiver {
                        crate::invariant();
                    }
                    self.event += 1;
                    (sender, RoleLocalDirection::Local)
                } else {
                    self.side = 1;
                    (sender, RoleLocalDirection::Send)
                }
            } else {
                self.side = 0;
                self.event += 1;
                (receiver, RoleLocalDirection::Recv)
            };
            if reference_contains(reference, self.actor, self.membership) {
                return Some(ActorEvent { event, direction });
            }
        }
        None
    }
}
