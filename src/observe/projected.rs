use crate::{
    eff::EventPayload,
    global::role_program::{RoleLocalDirection, RoleProgram},
};

use super::{event::TapEvent, ids};

/// Read the compiled protocol supplied to a rendezvous.
///
/// The stream starts with [`PROJECTED_PROGRAM`](crate::runtime::tap::PROJECTED_PROGRAM)
/// and [`PROJECTED_ROLE`](crate::runtime::tap::PROJECTED_ROLE). It then contains
/// global source events, ordered scope markers, choice authorities, actor
/// bindings (when present), actor decisions, initial delegated-role bindings,
/// and local operation rows, in that order. Header counts determine the exact
/// extent of each section. Every section retains its original row order.
///
/// Local rows use the layouts of
/// [`PROJECTED_LOCAL`](crate::runtime::tap::PROJECTED_LOCAL),
/// [`PROJECTED_SEND`](crate::runtime::tap::PROJECTED_SEND), and
/// [`PROJECTED_RECV`](crate::runtime::tap::PROJECTED_RECV). The wire label is a 16-bit
/// source-event identity; the message label is the 8-bit label in `g::Msg`.
/// Save these observations alongside the executable to check frame headers
/// against the compiled protocol.
///
/// The iterator reads the same sealed image as execution. It allocates nothing,
/// does not use the bounded runtime tap ring, and does not change progress.
/// Static membership does not establish that an operation was enabled at a
/// particular point in an execution. Source structure and actor references
/// are immutable input for checking an execution history; this stream does not
/// contain the history's selected branches or roll visits.
///
/// ```
/// use hibana::{g, runtime::{program::project, tap}};
/// let global = g::send::<0, 1, g::Msg<7, u32>>();
/// let program = project::<0>(&global);
/// let row = tap::projected(&program)
///     .find(|row| row.id() == tap::PROJECTED_SEND).unwrap();
/// assert_eq!(row.id(), tap::PROJECTED_SEND);
/// assert_eq!(row.arg0() >> 16, 0); // wire label
/// assert_eq!(row.arg0() as u8, 7); // message label
/// ```
pub fn projected<const ROLE: u8>(
    program: &RoleProgram<ROLE>,
) -> impl ExactSizeIterator<Item = TapEvent> + Clone + use<ROLE> {
    let image = program.role_image_ref();
    let source = image.program();
    let events = source.columns.atom_count();
    let markers = source.source_scope_marker_count();
    let choices = source.route_resolver_row_count();
    let bindings = if source.has_actor_bindings() {
        events
    } else {
        0
    };
    let decisions = source.actor_decision_count();
    let initial = source.initial_actor_count();
    let key = u16::from(image.role()) << 8 | u16::from(ROLE);
    let count =
        2 + events + markers + choices + bindings + decisions + initial + image.local_step_count();
    (0..count).map(move |index| {
        if index == 0 {
            return TapEvent::new(
                1 | (u32::from(source.has_actor_bindings()) << 16),
                ids::PROJECTED_PROGRAM,
                key,
                (events as u32) << 16 | markers as u32,
                (decisions as u32) << 16 | initial as u32,
            );
        }
        if index == 1 {
            return TapEvent::new(
                image.local_step_count() as u32,
                ids::PROJECTED_ROLE,
                key,
                choices as u32,
                u32::from(source.facts.max_role),
            );
        }
        let mut row = index - 2;
        if row < events {
            let atom = source.event_atom_at(row);
            let payload_kind = u16::from(matches!(atom.payload, EventPayload::Endpoint(_)));
            return TapEvent::new(
                row as u32,
                ids::PROJECTED_SOURCE,
                payload_kind << 8 | u16::from(atom.origin.packed_bits()),
                u32::from(atom.from) << 24
                    | u32::from(atom.to) << 16
                    | u32::from(atom.lane) << 8
                    | u32::from(atom.label),
                atom.payload.encoded_value(),
            );
        }
        row -= events;
        if row < markers {
            let (scope, boundary, tag) = crate::invariant_some(source.source_marker_at(row));
            return TapEvent::new(
                row as u32,
                ids::PROJECTED_SCOPE,
                scope.raw(),
                u32::from(boundary),
                u32::from(tag),
            );
        }
        row -= markers;
        if row < choices {
            let (scope, resolver) =
                crate::invariant_some(source.route_resolver_authority_at_row(row));
            return TapEvent::new(
                row as u32,
                ids::PROJECTED_CHOICE,
                scope.raw(),
                u32::from(source.route_controller_role(scope)),
                resolver.map_or(0, |resolver| u32::from(resolver.resolver_id()) + 1),
            );
        }
        row -= choices;
        if row < bindings {
            let [sender, receiver] = crate::invariant_some(source.actor_binding_at(row));
            return TapEvent::new(
                row as u32,
                ids::PROJECTED_BINDING,
                key,
                u32::from(sender),
                u32::from(receiver),
            );
        }
        row -= bindings;
        if row < decisions {
            let (scope, [left, right]) = crate::invariant_some(source.actor_decision_at(row));
            return TapEvent::new(
                256 + row as u32,
                ids::PROJECTED_ACTOR_DECISION,
                scope.raw(),
                u32::from(left),
                u32::from(right),
            );
        }
        row -= decisions;
        if row < initial {
            let (role, actor) = crate::invariant_some(source.initial_actor_at(row));
            return TapEvent::new(
                row as u32,
                ids::PROJECTED_INITIAL_ACTOR,
                key,
                u32::from(role),
                u32::from(actor),
            );
        }
        row -= initial;
        let effect = image.local_step_effect(row).dense_ordinal();
        let atom = image.program().event_atom_at(effect);
        let id = match image.local_step_direction(row) {
            RoleLocalDirection::Local => ids::PROJECTED_LOCAL,
            RoleLocalDirection::Send => ids::PROJECTED_SEND,
            RoleLocalDirection::Recv => ids::PROJECTED_RECV,
        };
        let payload = match atom.payload {
            EventPayload::Wire(_) => 0,
            EventPayload::Endpoint(role) => 1 << 8 | u32::from(role),
        };
        TapEvent::new(
            row as u32,
            id,
            u16::from(image.role()) << 8 | u16::from(ROLE),
            (effect as u32) << 16 | u32::from(atom.lane) << 8 | u32::from(atom.label),
            u32::from(atom.from) << 24 | u32::from(atom.to) << 16 | payload,
        )
    })
}

#[cfg(all(test, hibana_repo_tests))]
mod tests;
