//! Read-only qualification observations. These values never drive operations
//! and are absent from the production library, including its public API.
use super::{Endpoint, RouteBranch, kernel::ResidentActor};
use crate::{global::typestate::LocalAction, test_support::transport::TestTransport};
use std::vec::Vec;

pub(crate) mod owned_future;

/// A copied header tag, not an Endpoint or a usable operation capability.
#[derive(Clone, Copy)]
pub(crate) struct ViewTagObservation {
    ptr: core::ptr::NonNull<super::carrier::KernelEndpointHeader>,
    handle: super::carrier::EndpointHandle,
    region: super::carrier::OperationRegion,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ActorCallCut {
    pub(crate) lease_available: bool,
    pub(crate) fault: Option<crate::rendezvous::SessionFaultKind>,
}

impl ViewTagObservation {
    /// Copy this stored value tag without treating it as usable authority.
    ///
    /// # Safety
    /// The original pinned header allocation must remain alive.
    pub(crate) unsafe fn value_identity(self) -> owned_future::ValueIdentity {
        // SAFETY: the caller retains the allocation, including a retired tag.
        unsafe { owned_future::identity(self.ptr, self.handle) }
    }

    /// Copy this actor's progress between polls, including while a different
    /// parallel borrow owns a pending future. The tag remains observation only.
    ///
    /// # Safety
    /// This exact pinned actor must remain alive, use `T`, and retain this view
    /// tag. No operation lease may be held during the observation.
    pub(crate) unsafe fn snapshot<T: crate::transport::Transport>(self) -> ActorSnapshot {
        // SAFETY: the caller fixes the erased transport and allocation lifetime.
        unsafe { snapshot_from_handle::<T>(self.ptr, self.handle, self.region) }.actor
    }

    /// Observe every resident operation plus this exact lexical borrow target.
    /// The target is separate from actor state: viewing the same actor through
    /// another Endpoint must not fabricate a protocol-state transition.
    ///
    /// # Safety
    /// The same lifetime, concrete transport and between-polls requirements as
    /// `snapshot` apply. This copied tag is never an operation capability.
    pub(crate) unsafe fn operation_cut<T: crate::transport::Transport>(
        self,
    ) -> OperationCutSnapshot {
        // SAFETY: the caller fixes the erased transport and allocation lifetime.
        unsafe { snapshot_from_handle::<T>(self.ptr, self.handle, self.region) }
    }

    /// Observe whether an existing view is an owned-send payload between polls.
    ///
    /// # Safety
    /// The pinned actor must remain alive and use exactly `T`. No operation
    /// lease may be held while this read-only observation acquires its lease.
    pub(crate) unsafe fn is_send_payload<T: crate::transport::Transport>(self) -> bool {
        // SAFETY: the caller establishes the concrete transport and lifetime.
        let header = unsafe { self.ptr.as_ref() };
        // SAFETY: this is the original actor allocation with concrete `T`.
        let actor = unsafe { header.actor().cast::<ResidentActor<'_, T>>().as_ref() };
        let _lease = actor
            .context
            .try_operation_lease()
            .expect("observation between polls");
        let view = actor
            .views
            .get(header.role())
            .expect("original view remains resident");
        // SAFETY: this lease excludes mutation of the resident operation.
        let phase = unsafe { (*view.primary_operation().state.get()).public_active_op };
        phase == super::kernel::PublicActiveOp::SendPayload
    }

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
    operation_tag(
        endpoint.owner.ptr,
        super::carrier::OperationHandle {
            endpoint: endpoint.owner.handle,
            region: super::carrier::OperationRegion::WHOLE,
        },
    )
}

pub(super) fn operation_tag(
    ptr: core::ptr::NonNull<super::carrier::KernelEndpointHeader>,
    handle: super::carrier::OperationHandle,
) -> ViewTagObservation {
    ViewTagObservation {
        ptr,
        handle: handle.endpoint,
        region: handle.region,
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
    /// Debug spelling of the view's primary operation phase, serialized to Lean
    /// only. Parallel lane slots require their own call-coverage observations.
    pub(crate) operation: std::string::String,
}

/// Every resident slot, including idle slots and non-primary parallel calls.
/// These copies are evidence only, never another operation/ownership registry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct OperationSnapshot {
    pub(crate) view: u8,
    pub(crate) lane: usize,
    pub(in crate::endpoint) phase: super::kernel::PublicActiveOp,
    /// Preparation can precede source selection. Do not invent a source row.
    pub(crate) preparing: Option<(u8, crate::eff::EventPayload)>,
    /// Exact existing commit plan: source event, logical label and lane.
    pub(crate) prepared_send: Option<(usize, u8, u8)>,
    pub(crate) offered_event: Option<usize>,
}

/// The actual Endpoint/parallel-borrow target used for this observation.
/// A resident slot does not retain this borrow region between polls.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct OperationTargetSnapshot {
    pub(crate) view: u8,
    pub(crate) lane: usize,
    pub(crate) whole: bool,
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) owned_lanes: Vec<usize>,
    pub(crate) other_active: bool,
    pub(crate) lane_mask_offset: Option<u16>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ActorSnapshot {
    pub(crate) completion_mutations: u64,
    pub(crate) actor: u8,
    pub(crate) run: u32,
    pub(crate) counter: u32,
    pub(crate) live_views: u16,
    /// The existing actor-local first fault, copied under the same lease.
    pub(crate) fault: Option<crate::rendezvous::SessionFaultKind>,
    pub(crate) views: Vec<ViewSnapshot>,
    pub(crate) steps: Vec<StepSnapshot>,
    /// Committed route history. Unknown choices remain unknown.
    pub(crate) choices: Vec<(u16, Option<u8>)>,
    /// Existing retained completion bits, separate from current event rows.
    /// These are copied under the same actor lease and never drive progress.
    pub(crate) retained_rolls: Vec<u16>,
}

impl ActorSnapshot {
    pub(crate) fn view_headers(&self) -> impl Iterator<Item = (u8, u32, u32)> + '_ {
        self.views
            .iter()
            .map(|view| (view.role, view.run, view.nonce))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct OperationCutSnapshot {
    pub(crate) actor: ActorSnapshot,
    pub(crate) operations: Vec<OperationSnapshot>,
    pub(crate) target: OperationTargetSnapshot,
}

/// Encode copied evidence only, after the direct localsides have run.
#[cfg(not(miri))]
pub(crate) fn operation_cut_source(cut: &OperationCutSnapshot) -> std::string::String {
    let views = cut
        .actor
        .views
        .iter()
        .map(|view| std::format!("{}", view.role))
        .collect::<Vec<_>>()
        .join(", ");
    let slots = cut
        .operations
        .iter()
        .map(|slot| {
            let mut phase = std::format!("{:?}", slot.phase);
            phase.get_mut(..1).unwrap().make_ascii_lowercase();
            std::format!(
                "{{ view := {}, lane := {}, phase := .{phase} }}",
                slot.view,
                slot.lane
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let target = &cut.target;
    std::format!(
        "{{ views := [{views}], slots := [{slots}], target := {{ \
        view := {}, lane := {}, whole := {}, start := {}, stop := {}, \
        ownedLanes := {:?}, otherActive := {}, laneMaskOffset := {} }} }}",
        target.view,
        target.lane,
        target.whole,
        target.start,
        target.end,
        target.owned_lanes,
        target.other_active,
        match target.lane_mask_offset {
            Some(offset) => std::format!("some {offset}"),
            None => std::string::String::from("none"),
        },
    )
}

/// Observe an offered branch before consuming its first operation.
///
/// # Safety
/// The branch must belong to an actor created with `TestTransport`.
pub(crate) unsafe fn branch_snapshot<const ROLE: u8>(
    branch: &RouteBranch<'_, '_, ROLE>,
) -> ActorSnapshot {
    let (ptr, handle) = branch.target.parts();
    // SAFETY: the caller establishes TestTransport and the preview retains the view.
    unsafe { snapshot_from_handle::<TestTransport>(ptr, handle.endpoint, handle.region) }.actor
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
    // SAFETY: this live Endpoint retains the erased TestTransport view.
    unsafe {
        snapshot_from_handle::<TestTransport>(
            endpoint.owner.ptr,
            endpoint.owner.handle,
            crate::endpoint::carrier::OperationRegion::WHOLE,
        )
        .actor
    }
}

unsafe fn snapshot_from_handle<T: crate::transport::Transport>(
    ptr: core::ptr::NonNull<super::carrier::KernelEndpointHeader>,
    handle: super::carrier::EndpointHandle,
    region: super::carrier::OperationRegion,
) -> OperationCutSnapshot {
    // SAFETY: the caller establishes T, and the live Endpoint retains
    // this pinned actor. Recover its original allocation pointer; do not widen
    // the narrow view borrow.
    let actor = unsafe {
        let header = ptr.as_ref();
        assert!(handle.matches_header(header));
        header.actor().cast::<ResidentActor<'_, T>>().as_ref()
    };
    let mut operation = actor
        .context
        .try_operation_lease()
        .expect("observation lease");
    // SAFETY: the caller retains this pinned view header for the observation.
    let role = unsafe { ptr.as_ref() }.role();
    actor.with_kernel(
        &mut operation,
        actor.views.get(role).unwrap(),
        region,
        |kernel| {
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
                                    .relocatable_resident_lane_step_at_index(
                                        index,
                                        usize::from(lane),
                                    )
                                    .unwrap(),
                                &mut |scope| kernel.selected_arm_for_scope(scope),
                            )
                            .map(|scope| scope.raw() % 8192),
                    });
                }
                index += 1;
            }
            assert_eq!(steps.len(), cursor.local_steps_len());
            let mut operations = Vec::new();
            let mut target_lane = None;
            for view in actor.views.as_slice() {
                for (lane, slot) in view.operations().enumerate() {
                    let current = core::ptr::eq(slot.state.get(), kernel.operation);
                    let state = if current {
                        target_lane = Some(lane);
                        &*kernel.operation
                    } else {
                        // SAFETY: the actor lease excludes other kernels and
                        // the current mutable operation cell was excluded.
                        unsafe { &*slot.state.get() }
                    };
                    operations.push(OperationSnapshot {
                        view: view.public_header.role(),
                        lane,
                        phase: state.public_active_op,
                        preparing: match state.public_send_state {
                            super::kernel::SendState::Preparing {
                                logical_label,
                                payload_kind,
                                ..
                            } => Some((logical_label, payload_kind)),
                            super::kernel::SendState::Sending { .. }
                            | super::kernel::SendState::Done => None,
                        },
                        prepared_send: state.proof_prepared_send(),
                        offered_event: state
                            .public_route_branch
                            .as_ref()
                            .map(|branch| branch.branch_meta.eff_index.dense_ordinal()),
                    });
                }
            }
            let bounds = region.bounds();
            let target = OperationTargetSnapshot {
                view: role,
                lane: target_lane.expect("current operation is resident"),
                whole: region.is_whole(),
                start: bounds.start,
                end: bounds.end,
                owned_lanes: actor
                    .views
                    .get(role)
                    .unwrap()
                    .operations()
                    .enumerate()
                    .filter_map(|(lane, _)| kernel.owns_lane(lane).then_some(lane))
                    .collect(),
                other_active: kernel.has_other_active_operations(),
                lane_mask_offset: (!region.is_whole()).then(|| region.mask()),
            };
            let snapshot = ActorSnapshot {
                completion_mutations: cursor.completion_mutations(),
                actor: cursor.role(),
                run: kernel.context.public_generation.get(),
                counter: kernel.context.view_generation.get(),
                live_views: kernel.context.live_views.get(),
                fault: kernel
                    .context
                    .session
                    .rendezvous()
                    .session_fault(kernel.context.sid),
                views: actor
                    .views
                    .as_slice()
                    .iter()
                    .map(|view| {
                        let header = &view.public_header;
                        let operation = if core::ptr::eq(
                            view.primary_operation().state.get(),
                            kernel.operation,
                        ) {
                            kernel.operation.public_active_op
                        } else {
                            // SAFETY: the actor operation lease excludes mutation,
                            // and this primary cell is not kernel.operation.
                            unsafe { &*view.primary_operation().state.get() }.public_active_op
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
                retained_rolls: cursor.retained_roll_scopes(),
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
            };
            OperationCutSnapshot {
                actor: snapshot,
                operations,
                target,
            }
        },
    )
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
    actor.with_kernel(
        &mut operation,
        actor.views.get(ROLE).unwrap(),
        crate::endpoint::carrier::OperationRegion::WHOLE,
        |kernel| {
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
        },
    )
}
