//! Read-only qualification observations. These values never drive operations
//! and are absent from the production library, including its public API.
use super::{Endpoint, RouteBranch, kernel::ResidentActor};
use crate::{global::typestate::LocalAction, test_support::transport::TestTransport};
use std::vec::Vec;

/// A copied header tag, not an Endpoint or a usable operation capability.
#[derive(Clone, Copy)]
pub(crate) struct ViewTagObservation {
    ptr: core::ptr::NonNull<super::carrier::KernelEndpointHeader>,
    handle: super::carrier::EndpointHandle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ActorCallCut {
    pub(crate) lease_available: bool,
    pub(crate) fault: Option<crate::rendezvous::SessionFaultKind>,
}

impl ViewTagObservation {
    /// # Safety
    /// The observed Endpoint's pinned runtime allocation must remain alive.
    pub(crate) unsafe fn matches_header(self) -> bool {
        // SAFETY: the caller retains the original pinned view record.
        self.handle.matches_header(unsafe { self.ptr.as_ref() })
    }

    /// Observe the existing public-call lease and first fault without borrowing
    /// progress. This is also valid inside the transport callback, where an
    /// actor snapshot cannot acquire its required mutation lease.
    ///
    /// # Safety
    /// The pinned actor must remain alive, and `T` must be the concrete
    /// transport used to create it. Endpoint erasure does not establish `T`.
    pub(crate) unsafe fn actor_call_cut<T: crate::transport::Transport>(self) -> ActorCallCut {
        // SAFETY: the caller fixes the erased transport and retains the
        // original pinned actor, even when this copied view tag is retired.
        let actor = unsafe {
            self.ptr
                .as_ref()
                .actor()
                .cast::<ResidentActor<'_, T>>()
                .as_ref()
        };
        let operation = actor.context.try_operation_lease();
        let cut = ActorCallCut {
            lease_available: operation.is_some(),
            fault: actor
                .context
                .session
                .rendezvous()
                .session_fault(actor.context.sid),
        };
        drop(operation);
        cut
    }
}

pub(crate) fn view_tag<const ROLE: u8>(endpoint: &Endpoint<'_, ROLE>) -> ViewTagObservation {
    ViewTagObservation {
        ptr: endpoint.owner.ptr,
        handle: endpoint.owner.handle,
    }
}

/// Copy the existing first fault without an Endpoint operation or progress.
///
/// # Safety
/// The Endpoint must belong to a pinned actor created with `TestTransport`.
pub(crate) unsafe fn actor_fault<const ROLE: u8>(
    endpoint: &Endpoint<'_, ROLE>,
) -> Option<crate::rendezvous::SessionFaultKind> {
    // SAFETY: the caller establishes the erased transport and actor lifetime.
    let actor = unsafe {
        let header = endpoint.owner.ptr.as_ref();
        assert!(endpoint.owner.handle.matches_header(header));
        header
            .actor()
            .cast::<ResidentActor<'_, TestTransport>>()
            .as_ref()
    };
    let _operation = actor
        .context
        .try_operation_lease()
        .expect("observation lease");
    actor
        .context
        .session
        .rendezvous()
        .session_fault(actor.context.sid)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct StepSnapshot {
    pub(crate) view: u8,
    pub(crate) action: LocalAction,
    pub(crate) done: bool,
    /// Existing cursor query for this candidate; observation, never authority.
    pub(crate) reentry_scope: Option<u16>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ViewSnapshot {
    pub(crate) role: u8,
    pub(crate) run: u32,
    pub(crate) nonce: u32,
    /// Debug spelling of the existing kernel phase, serialized to Lean only.
    pub(crate) operation: std::string::String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ActorSnapshot {
    pub(crate) completion_mutations: u64,
    pub(crate) actor: u8,
    pub(crate) run: u32,
    pub(crate) counter: u32,
    pub(crate) live_views: u16,
    pub(crate) views: Vec<ViewSnapshot>,
    pub(crate) steps: Vec<StepSnapshot>,
    /// Committed route history. Unknown choices remain unknown.
    pub(crate) choices: Vec<(u16, Option<u8>)>,
}

impl ActorSnapshot {
    pub(crate) fn view_headers(&self) -> impl Iterator<Item = (u8, u32, u32)> + '_ {
        self.views
            .iter()
            .map(|view| (view.role, view.run, view.nonce))
    }
}

/// Observe an offered branch before consuming its first operation.
///
/// # Safety
/// The branch must belong to an actor created with `TestTransport`.
pub(crate) unsafe fn branch_snapshot<const ROLE: u8>(
    branch: &RouteBranch<'_, '_, ROLE>,
) -> ActorSnapshot {
    // SAFETY: the live branch exclusively borrows its Endpoint, and the caller
    // establishes the concrete transport required by actor_snapshot.
    unsafe { actor_snapshot(&*branch.endpoint) }
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
                    reentry_scope: cursor
                        .roll_body_reentry_scope_for_step(
                            cursor
                                .relocatable_resident_lane_step_at_index(index, usize::from(lane))
                                .unwrap(),
                            &mut |scope| kernel.selected_arm_for_scope(scope),
                        )
                        .map(|scope| scope.raw() % 8192),
                });
            }
            index += 1;
        }
        assert_eq!(steps.len(), cursor.local_steps_len());
        ActorSnapshot {
            completion_mutations: cursor.completion_mutations(),
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
                    let operation = if core::ptr::eq(header, kernel.public_header) {
                        kernel.io.public_active_op
                    } else {
                        // SAFETY: the actor operation lease excludes mutation,
                        // and this view is distinct from kernel.io's borrow.
                        unsafe { &*view.io.get() }.public_active_op
                    };
                    ViewSnapshot {
                        role: header.role(),
                        run: header.generation(),
                        nonce: header.view_generation(),
                        operation: std::format!("{operation:?}"),
                    }
                })
                .collect(),
            steps,
            choices: (0..kernel.context.program.route_resolver_row_count())
                .filter_map(|row| {
                    let (scope, _) = kernel
                        .context
                        .program
                        .route_resolver_authority_at_row(row)
                        .unwrap();
                    // A retained slot can have no local events in either
                    // arm. It still has an allocated choice cell, whose
                    // unknown value is part of this actor's copied state.
                    cursor
                        .route_scope_slot(scope)
                        .is_some()
                        .then(|| (scope.raw() % 8192, kernel.selected_arm_for_scope(scope)))
                })
                .collect(),
        }
    })
}

/// Observe the production cursor's admission for every descriptor row without
/// choosing a route, polling I/O, or advancing progress. These queries describe
/// descriptor readiness, not ownership of a Rust value or transport readiness.
///
/// # Safety
/// The Endpoint must belong to a pinned actor created with `TestTransport`.
pub(crate) unsafe fn actor_admissions<const ROLE: u8>(endpoint: &Endpoint<'_, ROLE>) -> Vec<bool> {
    // SAFETY: the caller fixes the erased transport and retains the actor.
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
        (0..cursor.local_steps_len())
            .map(|index| {
                let meta = match cursor.action_at(index) {
                    LocalAction::Send { .. } => cursor.try_send_meta_at(index).unwrap().into(),
                    LocalAction::Recv { .. } => cursor.try_recv_meta_at(index).unwrap().into(),
                    LocalAction::Local { .. } => {
                        let meta = cursor.try_local_meta_at(index).unwrap();
                        crate::global::typestate::EventCommitMeta::new(
                            meta.eff_index,
                            meta.label,
                            meta.origin,
                            meta.scope,
                            meta.route_arm,
                            meta.lane,
                        )
                    }
                    LocalAction::Terminate => crate::invariant(),
                };
                cursor
                    .event_enabled(index, meta, &mut |scope, _| {
                        kernel.selected_arm_for_scope(scope)
                    })
                    .is_ok()
            })
            .collect()
    })
}
