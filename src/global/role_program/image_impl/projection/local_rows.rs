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
    program: CompiledProgramRef,
    resources: [u8; 32],
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
            program: *program,
            resources: [0; 32],
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
            if let crate::eff::EventPayload::Endpoint(role) = program.event_atom_at(event).payload {
                rows.resources[role as usize / 8] |= 1 << (role % 8);
            }
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

pub(crate) struct RoleLocalRows<const E: usize> {
    role: u8,
    starts: [u16; E],
    events: usize,
    len: u16,
}

impl<const E: usize> RoleLocalRows<E> {
    pub(crate) const fn new(effects: &EffList<E>, role: u8) -> Self {
        let mut rows = Self {
            role,
            starts: [0; E],
            events: effects.len(),
            len: 0,
        };
        let mut event = 0;
        while event < rows.events {
            rows.starts[event] = rows.len;
            let atom = effects.atom_at(event);
            if atom.from == role || atom.to == role {
                rows.len = match rows.len.checked_add(1) {
                    Some(len) => len,
                    None => panic!("role local operation count exceeds compact domain"),
                };
            }
            event += 1;
        }
        rows
    }
}

/// Both projections retain immutable prefix counts during image construction.
/// These compiler rows are never stored in a runtime endpoint.
#[derive(Clone, Copy)]
pub(crate) enum LocalRows<'a, const E: usize> {
    Role(&'a RoleLocalRows<E>),
    Actor(&'a ActorLocalRows<E>),
}

impl<const E: usize> LocalRows<'_, E> {
    /// The first source occurrence reads the resource's entry owner: no
    /// preceding occurrence in this fragment could have transferred it.
    /// Enumeration here constructs immutable facts, never an execution order.
    pub(in crate::global::role_program::image_impl) const fn roll_entry_actor(
        self,
        effects: &EffList<E>,
        start: usize,
        end: usize,
        role: u8,
    ) -> Option<u16> {
        let Self::Actor(rows) = self else {
            return None;
        };
        if rows.resources[role as usize / 8] & (1 << (role % 8)) == 0 {
            return None;
        }
        let mut event = start;
        while event < end {
            let atom = effects.atom_at(event);
            let side = if atom.from == role
                || matches!(atom.payload,
                crate::eff::EventPayload::Endpoint(key) if key == role)
            {
                0
            } else if atom.to == role {
                1
            } else {
                event += 1;
                continue;
            };
            let Some(bindings) = rows.program.actor_binding_at(event) else {
                crate::invariant();
            };
            let direction = if atom.from == atom.to {
                RoleLocalDirection::Local
            } else if side == 0 {
                RoleLocalDirection::Send
            } else {
                RoleLocalDirection::Recv
            };
            // The existing actor projection already computed exact possible
            // membership. Another actor's entry view is not a local obligation.
            if rows.directions[event] & (1 << direction as u8) == 0 {
                return None;
            }
            return Some(bindings[side]);
        }
        None
    }

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
            Self::Role(rows) => rows.role,
            Self::Actor(rows) => rows.actor,
        }
    }

    pub(crate) const fn count(self, effects: &EffList<E>, event: usize) -> usize {
        match self {
            Self::Role(rows) => {
                if event >= rows.events || rows.events != effects.len() {
                    crate::invariant();
                }
                let end = if event + 1 == rows.events {
                    rows.len
                } else {
                    rows.starts[event + 1]
                };
                (end - rows.starts[event]) as usize
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
            Self::Role(rows) => {
                if side != 0 {
                    crate::invariant();
                }
                let atom = effects.atom_at(event);
                match role_local_direction(rows.role, atom.from, atom.to) {
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
            Self::Role(rows) => {
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

#[cfg(all(test, hibana_repo_tests))]
mod tests {
    use super::*;
    use crate::eff::{EffAtom, EventOrigin, EventPayload};

    #[test]
    fn prefix_ranges_match_direct_counting_for_every_small_membership_mask() {
        for mask in 0u16..256 {
            let mut source = EffList::<8>::new();
            for event in 0..8 {
                let (from, to) = if mask & (1 << event) == 0 {
                    (1, 2)
                } else {
                    match event % 3 {
                        0 => (0, 1),
                        1 => (1, 0),
                        _ => (0, 0),
                    }
                };
                source.push_event_mut(EffAtom {
                    from,
                    to,
                    label: event,
                    lane: 0,
                    payload: EventPayload::Wire(0),
                    origin: EventOrigin::User,
                });
            }
            for role in [0, 1, 2, 255] {
                let index = RoleLocalRows::new(&source, role);
                let rows = LocalRows::Role(&index);
                let participates = |event| {
                    let atom = source.atom_at(event);
                    atom.from == role || atom.to == role
                };
                for start in 0..=8 {
                    for end in start..=8 {
                        let before = (0..start).filter(|&event| participates(event)).count();
                        let length = (start..end).filter(|&event| participates(event)).count();
                        let actual = rows.range(&source, start, end);
                        assert_eq!(
                            actual.len(),
                            length,
                            "mask={mask} role={role} {start}..{end}"
                        );
                        assert_eq!(actual.start(), if length == 0 { 0 } else { before });
                    }
                }
            }
        }
    }

    #[test]
    fn an_empty_source_has_one_empty_boundary_and_no_event_lookup() {
        let source = EffList::<0>::new();
        let index = RoleLocalRows::new(&source, 255);
        let range = LocalRows::Role(&index).range(&source, 0, 0);
        assert_eq!((range.start(), range.len()), (0, 0));
    }
}
