//! Construction-only selection of logical operations for a projected actor.
//!
//! The map is immutable compiler workspace. It is never attached to an endpoint
//! or consulted as a runtime permission table. Logical atoms and scope identities
//! stay in the original global image.
use super::super::event_rows::{RoleLocalDirection, role_local_direction};
use super::{super::super::PackedLaneRange, ActorEventProjection};
use crate::global::{
    compiled::images::CompiledProgramRef,
    const_dsl::{EffList, ScopeId, ScopeKind},
};

const CONTEXT_BYTES: usize = (ScopeId::LOCAL_CAPACITY as usize).div_ceil(8);

pub(crate) struct ActorLocalRows<const E: usize> {
    actor: u8,
    directions: [u8; E],
    starts: [u16; E],
    events: usize,
    len: u16,
    contexts: [u8; CONTEXT_BYTES],
}

impl<const E: usize> ActorLocalRows<E> {
    pub(crate) const fn new(program: &CompiledProgramRef, actor: u8) -> Self {
        if program.columns.atom_count() > E {
            crate::invariant();
        }
        let mut rows = Self {
            actor,
            directions: [0; E],
            starts: [0; E],
            events: program.columns.atom_count(),
            len: 0,
            contexts: [0; CONTEXT_BYTES],
        };
        if program.actor_decision_count() == 0 {
            rows.collect(program, &mut []);
        } else {
            const BYTES: usize =
                crate::global::compiled::images::ACTOR_DECISION_CAPACITY.div_ceil(8);
            rows.collect(program, &mut [0; BYTES]);
        }
        let mut event = 0;
        while event < rows.events {
            rows.starts[event] = rows.len;
            let count = rows.directions[event].count_ones() as u16;
            rows.len = match rows.len.checked_add(count) {
                Some(len) => len,
                None => panic!("actor local operation count exceeds compact domain"),
            };
            event += 1;
        }
        rows
    }

    const fn collect(&mut self, program: &CompiledProgramRef, scratch: &mut [u8]) {
        {
            let Some(mut projection) = ActorEventProjection::new(program, self.actor, scratch)
            else {
                crate::invariant();
            };
            while let Some(row) = projection.next() {
                let bit = 1 << row.direction as u8;
                if self.directions[row.event] & bit != 0 {
                    crate::invariant();
                }
                self.directions[row.event] |= bit;
            }
        }
        // Reuse the membership scratch to mark exactly the decision ancestors
        // needed by local operation or peer bindings. Keep original route IDs
        // even when the guarded use lies after that route's join.
        let decisions = program.actor_decision_count();
        let mut index = 0;
        while index < decisions.div_ceil(8) {
            scratch[index] = 0;
            index += 1;
        }
        let mut event = 0;
        while event < self.events {
            if self.directions[event] != 0
                && let Some([sender, receiver]) = program.actor_binding_at(event)
            {
                Self::retain_reference(sender, scratch);
                Self::retain_reference(receiver, scratch);
            }
            event += 1;
        }
        index = decisions;
        while index != 0 {
            index -= 1;
            if scratch[index / 8] & (1 << (index % 8)) != 0 {
                let Some((scope, [left, right])) = program.actor_decision_at(index) else {
                    crate::invariant();
                };
                let scope = scope.local_ordinal() as usize;
                self.contexts[scope / 8] |= 1 << (scope % 8);
                Self::retain_reference(left, scratch);
                Self::retain_reference(right, scratch);
            }
        }
    }

    const fn retain_reference(reference: u16, scratch: &mut [u8]) {
        if reference >= 256 {
            let index = reference as usize - 256;
            if index / 8 >= scratch.len() {
                crate::invariant();
            }
            scratch[index / 8] |= 1 << (index % 8);
        }
    }
}

/// Identity bindings require no per-event workspace. Delegation uses the same
/// compiler algorithms with an actor's immutable candidate rows.
#[derive(Clone, Copy)]
pub(crate) enum LocalRows<'a, const E: usize> {
    Role(u8),
    Actor(&'a ActorLocalRows<E>),
}

impl<const E: usize> LocalRows<'_, E> {
    pub(crate) const fn retains_context(self, scope: ScopeId) -> bool {
        if !matches!(scope.kind(), Some(ScopeKind::Route)) {
            return false;
        }
        match self {
            Self::Role(_) => false,
            Self::Actor(rows) => {
                let ordinal = scope.local_ordinal() as usize;
                rows.contexts[ordinal / 8] & (1 << (ordinal % 8)) != 0
            }
        }
    }

    pub(crate) const fn actor(self) -> u8 {
        match self {
            Self::Role(role) => role,
            Self::Actor(rows) => rows.actor,
        }
    }

    pub(crate) const fn count(self, effects: &EffList<E>, event: usize) -> usize {
        match self {
            Self::Role(role) => {
                let atom = effects.atom_at(event);
                (atom.from == role || atom.to == role) as usize
            }
            Self::Actor(rows) => {
                if event >= rows.events || rows.events != effects.len() {
                    crate::invariant();
                }
                rows.directions[event].count_ones() as usize
            }
        }
    }

    pub(crate) const fn direction(
        self,
        effects: &EffList<E>,
        event: usize,
        side: usize,
    ) -> RoleLocalDirection {
        match self {
            Self::Role(role) => {
                if side != 0 {
                    crate::invariant();
                }
                let atom = effects.atom_at(event);
                match role_local_direction(role, atom.from, atom.to) {
                    Some(direction) => direction,
                    None => crate::invariant(),
                }
            }
            Self::Actor(rows) => {
                if side >= self.count(effects, event) {
                    crate::invariant();
                }
                let mut mask = rows.directions[event];
                let mut index = 0;
                while index < side {
                    mask &= mask - 1;
                    index += 1;
                }
                match mask.trailing_zeros() {
                    0 => RoleLocalDirection::Local,
                    1 => RoleLocalDirection::Send,
                    2 => RoleLocalDirection::Recv,
                    _ => crate::invariant(),
                }
            }
        }
    }

    pub(crate) const fn range(
        self,
        effects: &EffList<E>,
        start: usize,
        end: usize,
    ) -> PackedLaneRange {
        if start > end || end > effects.len() {
            crate::invariant();
        }
        let (start, len) = match self {
            Self::Role(role) => {
                let mut before = 0;
                let mut len = 0;
                let mut event = 0;
                while event < end {
                    let atom = effects.atom_at(event);
                    if atom.from == role || atom.to == role {
                        if event < start {
                            before += 1;
                        } else {
                            len += 1;
                        }
                    }
                    event += 1;
                }
                (before, len)
            }
            Self::Actor(rows) => {
                if rows.events != effects.len() {
                    crate::invariant();
                }
                let first = if start == rows.events {
                    rows.len
                } else {
                    rows.starts[start]
                };
                let stop = if end == rows.events {
                    rows.len
                } else {
                    rows.starts[end]
                };
                (first as usize, (stop - first) as usize)
            }
        };
        PackedLaneRange::new(if len == 0 { 0 } else { start }, len)
    }
}
