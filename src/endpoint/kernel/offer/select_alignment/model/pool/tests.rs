use super::*;
use crate::endpoint::kernel::frontier::{
    ExactOfferObservation, FrontierKind, FrontierObservationSlot, ObservedEntrySetBuilder,
    OfferEntryAdmission, OfferEntryKey, OfferEntryObservedState,
};
use crate::endpoint::kernel::offer::select_alignment::model::{
    CurrentOfferAuthority, CurrentOfferCandidateStatus, CurrentOfferEntry, OfferAlignmentDecision,
    ProgressSiblingPresence,
};
use crate::global::{const_dsl::ScopeId, typestate::StateIndex};

fn observed(entry: u16, scope: u16, frontier: FrontierKind, flags: u8) -> OfferEntryObservedState {
    OfferEntryObservedState {
        key: OfferEntryKey::new(ScopeId::route(scope), StateIndex::new(entry))
            .expect("exact observation key"),
        frontier_mask: frontier.bit(),
        flags,
    }
}

fn current_observation(
    entry: u16,
    scope: u16,
    frontier: FrontierKind,
    flags: u8,
    admission: OfferEntryAdmission,
) -> CurrentOfferObservation {
    let row = observed(entry, scope, frontier, flags);
    let exact = ExactOfferObservation::from_target(row.key, row, admission);
    CurrentOfferObservation::from_exact(exact)
}

fn candidate_input(
    current_idx: usize,
    current_observation: CurrentOfferObservation,
) -> OfferAlignmentCandidateInput {
    OfferAlignmentCandidateInput {
        current_idx,
        current_entry: CurrentOfferEntry::RouteWithOfferLanes,
        current_authority: CurrentOfferAuthority::Passive,
        progress_sibling_presence: ProgressSiblingPresence::Absent,
        current_observation,
    }
}

#[cfg(all(test, hibana_repo_tests))]
#[test]
fn same_target_entry_rows_do_not_synthesize_controller_progress() {
    let mut storage = [FrontierObservationSlot::EMPTY; 2];
    let mut rows = ObservedEntrySetBuilder::from_slice(&mut storage);
    rows.clear();
    rows.push_exact_observation(
        observed(
            7,
            1,
            FrontierKind::Route,
            OfferEntryObservedState::FLAG_CONTROLLER,
        ),
        OfferEntryAdmission::Selectable,
    );
    rows.push_exact_observation(
        observed(
            7,
            2,
            FrontierKind::Reentry,
            OfferEntryObservedState::FLAG_PROGRESS,
        ),
        OfferEntryAdmission::Selectable,
    );

    let selection = OfferAlignmentCandidatePool::from_observed(
        rows.seal(),
        candidate_input(99, CurrentOfferObservation::empty()),
    )
    .selection();
    assert_eq!(
        selection.select(CurrentOfferCandidateStatus::NotSelectable, 99),
        Some(OfferAlignmentDecision::Realign(7))
    );
}

#[cfg(all(test, hibana_repo_tests))]
#[test]
fn current_target_readiness_requires_one_selectable_exact_witness() {
    let mut storage = [FrontierObservationSlot::EMPTY; 2];
    let mut rows = ObservedEntrySetBuilder::from_slice(&mut storage);
    rows.clear();
    rows.push_exact_observation(
        observed(7, 1, FrontierKind::Route, 0),
        OfferEntryAdmission::Selectable,
    );
    rows.push_exact_observation(
        observed(
            7,
            2,
            FrontierKind::Reentry,
            OfferEntryObservedState::FLAG_PROGRESS | OfferEntryObservedState::FLAG_READY,
        ),
        OfferEntryAdmission::Excluded,
    );

    let exact_current = current_observation(
        7,
        1,
        FrontierKind::Route,
        0,
        OfferEntryAdmission::Selectable,
    );
    let observation =
        OfferAlignmentCandidatePool::from_observed(rows.seal(), candidate_input(7, exact_current))
            .current_observation();
    assert!(observation.present());
    assert!(observation.selectable());
    assert!(!observation.ready());
    assert!(!observation.progress_evidence());
}

#[cfg(all(test, hibana_repo_tests))]
#[test]
fn same_entry_other_scope_cannot_authorize_absent_current_scope() {
    let mut storage = [FrontierObservationSlot::EMPTY; 1];
    let mut rows = ObservedEntrySetBuilder::from_slice(&mut storage);
    rows.clear();
    rows.push_exact_observation(
        observed(
            7,
            2,
            FrontierKind::Reentry,
            OfferEntryObservedState::FLAG_PROGRESS
                | OfferEntryObservedState::FLAG_READY
                | OfferEntryObservedState::FLAG_READY_ARM,
        ),
        OfferEntryAdmission::Selectable,
    );

    let pool = OfferAlignmentCandidatePool::from_observed(
        rows.seal(),
        candidate_input(7, CurrentOfferObservation::empty()),
    );
    let observation = pool.current_observation();
    assert!(!observation.present());
    assert!(!observation.selectable());
    assert!(!observation.ready());
    assert!(!observation.progress_evidence());
    assert_eq!(
        pool.selection()
            .select(CurrentOfferCandidateStatus::NotSelectable, 7),
        None
    );
}

#[cfg(all(test, hibana_repo_tests))]
#[test]
fn excluded_exact_current_scope_is_not_selectable() {
    let current = current_observation(
        7,
        1,
        FrontierKind::Route,
        OfferEntryObservedState::FLAG_PROGRESS
            | OfferEntryObservedState::FLAG_READY
            | OfferEntryObservedState::FLAG_READY_ARM,
        OfferEntryAdmission::Excluded,
    );
    assert!(current.present());
    assert!(!current.selectable());
    assert!(!current.ready());
    assert!(!current.progress_evidence());
    assert!(!current.permits_retention());
}

#[cfg(all(test, hibana_repo_tests))]
#[test]
fn absent_exact_current_scope_does_not_override_descriptor_retention() {
    let current = CurrentOfferObservation::empty();
    assert!(!current.present());
    assert!(current.permits_retention());
}

#[cfg(kani)]
#[kani::proof]
fn exact_scope_rows_never_synthesize_controller_progress_authority() {
    let mut storage = [FrontierObservationSlot::EMPTY; 2];
    let mut rows = ObservedEntrySetBuilder::from_slice(&mut storage);
    rows.clear();
    rows.push_exact_observation(
        observed(
            7,
            1,
            FrontierKind::Route,
            OfferEntryObservedState::FLAG_CONTROLLER,
        ),
        OfferEntryAdmission::Selectable,
    );
    rows.push_exact_observation(
        observed(
            7,
            2,
            FrontierKind::Reentry,
            OfferEntryObservedState::FLAG_PROGRESS,
        ),
        OfferEntryAdmission::Selectable,
    );

    let selection = OfferAlignmentCandidatePool::from_observed(
        rows.seal(),
        candidate_input(99, CurrentOfferObservation::empty()),
    )
    .selection();
    assert_eq!(
        selection.select(CurrentOfferCandidateStatus::NotSelectable, 99),
        Some(OfferAlignmentDecision::Realign(7))
    );
}

#[cfg(kani)]
#[kani::proof]
fn same_entry_erased_flags_cannot_change_exact_current_observation() {
    let ready: bool = kani::any();
    let progress: bool = kani::any();
    let ready_arm: bool = kani::any();
    let mut flags = 0u8;
    if ready {
        flags |= OfferEntryObservedState::FLAG_READY;
    }
    if progress {
        flags |= OfferEntryObservedState::FLAG_PROGRESS;
    }
    if ready_arm {
        flags |= OfferEntryObservedState::FLAG_READY_ARM;
    }

    let mut storage = [FrontierObservationSlot::EMPTY; 1];
    let mut rows = ObservedEntrySetBuilder::from_slice(&mut storage);
    rows.clear();
    rows.push_exact_observation(
        observed(7, 2, FrontierKind::Reentry, flags),
        OfferEntryAdmission::Selectable,
    );

    let pool = OfferAlignmentCandidatePool::from_observed(
        rows.seal(),
        candidate_input(7, CurrentOfferObservation::empty()),
    );
    let current = pool.current_observation();
    assert!(!current.present());
    assert!(!current.selectable());
    assert!(!current.ready());
    assert!(!current.progress_evidence());
    assert!(current.permits_retention());
    assert_eq!(
        pool.selection()
            .select(CurrentOfferCandidateStatus::NotSelectable, 7),
        None
    );
}

#[cfg(kani)]
#[kani::proof]
fn excluded_exact_current_never_permits_retention() {
    let ready: bool = kani::any();
    let progress: bool = kani::any();
    let ready_arm: bool = kani::any();
    let mut flags = 0u8;
    if ready {
        flags |= OfferEntryObservedState::FLAG_READY;
    }
    if progress {
        flags |= OfferEntryObservedState::FLAG_PROGRESS;
    }
    if ready_arm {
        flags |= OfferEntryObservedState::FLAG_READY_ARM;
    }

    let current = current_observation(
        7,
        1,
        FrontierKind::Route,
        flags,
        OfferEntryAdmission::Excluded,
    );
    assert!(current.present());
    assert!(!current.selectable());
    assert!(!current.permits_retention());
}

#[cfg(all(test, hibana_repo_tests))]
#[test]
fn a_local_resolver_can_progress_without_inventing_ingress_authority() {
    let mut storage = [FrontierObservationSlot::EMPTY; 2];
    let mut rows = ObservedEntrySetBuilder::from_slice(&mut storage);
    rows.clear();
    let policy = observed(
        7,
        1,
        FrontierKind::Route,
        OfferEntryObservedState::FLAG_CONTROLLER
            | OfferEntryObservedState::FLAG_DYNAMIC
            | OfferEntryObservedState::FLAG_READY,
    );
    assert!(!policy.has_progress_evidence());
    assert!(!policy.has_ready_arm_evidence());
    rows.push_exact_observation(policy, OfferEntryAdmission::Selectable);
    rows.push_exact_observation(
        observed(9, 2, FrontierKind::Route, 0),
        OfferEntryAdmission::Selectable,
    );
    let pool = OfferAlignmentCandidatePool::from_observed(
        rows.seal(),
        candidate_input(
            9,
            current_observation(
                9,
                2,
                FrontierKind::Route,
                0,
                OfferEntryAdmission::Selectable,
            ),
        ),
    );
    assert_eq!(
        pool.selection()
            .select(CurrentOfferCandidateStatus::NotSelectable, 9),
        Some(OfferAlignmentDecision::Realign(7))
    );
}

#[cfg(all(test, hibana_repo_tests))]
#[test]
fn parallel_local_resolvers_choose_work_without_realigning_back_and_forth() {
    let flags = OfferEntryObservedState::FLAG_CONTROLLER
        | OfferEntryObservedState::FLAG_DYNAMIC
        | OfferEntryObservedState::FLAG_READY;
    let mut storage = [FrontierObservationSlot::EMPTY; 2];
    let mut rows = ObservedEntrySetBuilder::from_slice(&mut storage);
    rows.clear();
    rows.push_exact_observation(
        observed(7, 1, FrontierKind::Route, flags),
        OfferEntryAdmission::Selectable,
    );
    rows.push_exact_observation(
        observed(9, 2, FrontierKind::Route, flags),
        OfferEntryAdmission::Selectable,
    );
    let observations = rows.seal();
    let first = OfferAlignmentCandidatePool::from_observed(
        observations,
        candidate_input(99, CurrentOfferObservation::empty()),
    );
    assert_eq!(
        first
            .selection()
            .select(CurrentOfferCandidateStatus::NotSelectable, 99),
        Some(OfferAlignmentDecision::Realign(7))
    );
    let mut input = candidate_input(
        7,
        current_observation(
            7,
            1,
            FrontierKind::Route,
            flags,
            OfferEntryAdmission::Selectable,
        ),
    );
    input.current_authority = CurrentOfferAuthority::DynamicController;
    let current = OfferAlignmentCandidatePool::from_observed(observations, input);
    assert!(current.selection().allows_current(7));
    assert_eq!(
        current
            .selection()
            .select(CurrentOfferCandidateStatus::Selectable, 7),
        Some(OfferAlignmentDecision::KeepCurrent)
    );
}

#[cfg(kani)]
#[kani::proof]
#[kani::unwind(8)]
fn resolver_readiness_preserves_exact_admission_without_synthesizing_authority() {
    let selectable: bool = kani::any();
    let controller: bool = kani::any();
    let dynamic: bool = kani::any();
    let ready: bool = kani::any();
    let mut flags = 0;
    if controller {
        flags |= OfferEntryObservedState::FLAG_CONTROLLER;
    }
    if dynamic {
        flags |= OfferEntryObservedState::FLAG_DYNAMIC;
    }
    if ready {
        flags |= OfferEntryObservedState::FLAG_READY;
    }
    let row = observed(7, 1, FrontierKind::Route, flags);
    assert!(!row.has_progress_evidence());
    assert!(!row.has_ready_arm_evidence());
    let mut storage = [FrontierObservationSlot::EMPTY; 1];
    let mut rows = ObservedEntrySetBuilder::from_slice(&mut storage);
    rows.clear();
    rows.push_exact_observation(
        row,
        if selectable {
            OfferEntryAdmission::Selectable
        } else {
            OfferEntryAdmission::Excluded
        },
    );
    let pool = OfferAlignmentCandidatePool::from_observed(
        rows.seal(),
        candidate_input(99, CurrentOfferObservation::empty()),
    );
    let selected = pool
        .selection()
        .select(CurrentOfferCandidateStatus::NotSelectable, 99);
    if !selectable {
        assert_eq!(selected, None);
    }
    if selectable && controller && dynamic && ready {
        assert_eq!(selected, Some(OfferAlignmentDecision::Realign(7)));
    }
}
