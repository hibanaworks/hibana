//! Read-only qualification observations. These values never drive operations
//! and are absent from the production library, including its public API.
use super::{Endpoint, kernel::ResidentActor};
use crate::{global::typestate::LocalAction, test_support::transport::TestTransport};
use std::vec::Vec;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct StepSnapshot {
    pub(crate) view: u8,
    pub(crate) action: LocalAction,
    pub(crate) done: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ActorSnapshot {
    pub(crate) actor: u8,
    pub(crate) run: u32,
    pub(crate) counter: u32,
    pub(crate) live_views: u16,
    /// (logical role, live run generation, actor-local view nonce).
    pub(crate) views: Vec<(u8, u32, u32)>,
    pub(crate) steps: Vec<StepSnapshot>,
}

/// Copy observations under the existing operation lease and kernel borrow.
/// No borrow or pointer is retained across a subsequent Endpoint operation.
///
/// # Safety
/// The Endpoint must belong to an actor created with `TestTransport`. Endpoints
/// erase that concrete type, so the caller must establish it at attachment.
pub(crate) unsafe fn actor_snapshot<const ROLE: u8>(
    endpoint: &Endpoint<'_, ROLE>,
) -> ActorSnapshot {
    // SAFETY: the caller establishes TestTransport, and the live Endpoint retains
    // this pinned actor. Recover its original allocation pointer; do not widen
    // the narrow view borrow.
    let actor = unsafe {
        let header = endpoint.owner.ptr.as_ref();
        assert!(endpoint.owner.handle.matches_header(header));
        header
            .actor()
            .cast::<ResidentActor<'_, TestTransport>>()
            .as_ref()
    };
    let mut operation = actor
        .context
        .try_operation_lease()
        .expect("observation lease");
    actor.with_kernel(&mut operation, actor.views.get(ROLE).unwrap(), |kernel| {
        let cursor = &kernel.progress.cursor;
        let mut steps = Vec::new();
        let mut index = 0;
        while cursor.contains_node_index(index) {
            let action = cursor.action_at(index);
            let lane = match action {
                LocalAction::Send { lane, .. }
                | LocalAction::Recv { lane, .. }
                | LocalAction::Local { lane, .. } => Some(lane),
                LocalAction::Terminate => None,
            };
            if let Some(lane) = lane {
                steps.push(StepSnapshot {
                    view: cursor.logical_role_at(index).expect("operation view"),
                    action,
                    done: cursor.node_event_done_for_lane(index, lane),
                });
            }
            index += 1;
        }
        assert_eq!(steps.len(), cursor.local_steps_len());
        ActorSnapshot {
            actor: cursor.role(),
            run: kernel.context.public_generation.get(),
            counter: kernel.context.view_generation.get(),
            live_views: kernel.context.live_views.get(),
            views: actor
                .views
                .as_slice()
                .iter()
                .map(|view| {
                    let header = &view.public_header;
                    (header.role(), header.generation(), header.view_generation())
                })
                .collect(),
            steps,
        }
    })
}
