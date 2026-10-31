use crate::{
    eff::EventPayload,
    global::role_program::{RoleLocalDirection, RoleProgram},
};

use super::{event::TapEvent, ids};

/// Inspect the compiled operations of the program supplied to a rendezvous.
///
/// Each item describes one local row, in storage order, using the layouts of
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
/// particular point in an execution. Payload schemas and dynamic route/roll
/// choices are outside these observations.
///
/// ```
/// use hibana::{g, runtime::{program::project, tap}};
/// let global = g::send::<0, 1, g::Msg<7, u32>>();
/// let program = project::<0>(&global);
/// let row = tap::projected(&program).next().unwrap();
/// assert_eq!(row.id(), tap::PROJECTED_SEND);
/// assert_eq!(row.arg0() >> 16, 0); // wire label
/// assert_eq!(row.arg0() as u8, 7); // message label
/// ```
pub fn projected<const ROLE: u8>(
    program: &RoleProgram<ROLE>,
) -> impl ExactSizeIterator<Item = TapEvent> + use<ROLE> {
    let image = program.role_image_ref();
    (0..image.local_step_count()).map(move |row| {
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
