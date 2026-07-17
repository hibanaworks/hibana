use super::*;
use crate::endpoint::kernel::frontier::{
    FrontierKind, FrontierObservationSlot, ObservedEntrySetBuilder, OfferEntryAdmission,
    OfferEntryKey, OfferEntryObservedState,
};
use crate::endpoint::kernel::offer::OfferSelectPriority;
use crate::endpoint::kernel::offer::select_alignment::model::{
    CurrentOfferAuthority, CurrentOfferCandidateStatus, CurrentOfferEntry, ProgressSiblingPresence,
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

fn candidate_input(current_idx: usize) -> OfferAlignmentCandidateInput {
    OfferAlignmentCandidateInput {
        current_idx,
        current_entry: CurrentOfferEntry::RouteWithOfferLanes,
        current_authority: CurrentOfferAuthority::Passive,
        progress_sibling_presence: ProgressSiblingPresence::Absent,
    }
}

#[cfg(all(test, hibana_repo_tests))]
#[test]
fn same_target_entry_rows_do_not_synthesize_controller_progress() {
    let mut storage = [FrontierObservationSlot::EMPTY; 2];
    /* SAFETY: the builder exclusively owns the complete initialized test
    storage until `seal` consumes it below. */
    let mut rows =
        unsafe { ObservedEntrySetBuilder::from_parts(storage.as_mut_ptr(), storage.len()) };
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

    let selection =
        OfferAlignmentCandidatePool::from_observed(rows.seal(), candidate_input(99)).selection();
    assert_eq!(
        selection.select(CurrentOfferCandidateStatus::NotSelectable, 99),
        Some((OfferSelectPriority::CandidateUnique, 7))
    );
}

#[cfg(all(test, hibana_repo_tests))]
#[test]
fn current_target_readiness_requires_one_selectable_exact_witness() {
    let mut storage = [FrontierObservationSlot::EMPTY; 2];
    /* SAFETY: the builder exclusively owns the complete initialized test
    storage until `seal` consumes it below. */
    let mut rows =
        unsafe { ObservedEntrySetBuilder::from_parts(storage.as_mut_ptr(), storage.len()) };
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

    let observation = OfferAlignmentCandidatePool::from_observed(rows.seal(), candidate_input(7))
        .current_observation();
    assert!(observation.present());
    assert!(!observation.ready());
    assert!(!observation.progress_evidence());
}

#[cfg(kani)]
#[kani::proof]
fn exact_scope_rows_never_synthesize_controller_progress_authority() {
    let mut storage = [FrontierObservationSlot::EMPTY; 2];
    /* SAFETY: the proof builder exclusively owns the complete initialized
    observation storage until `seal` consumes it below. */
    let mut rows =
        unsafe { ObservedEntrySetBuilder::from_parts(storage.as_mut_ptr(), storage.len()) };
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

    let selection =
        OfferAlignmentCandidatePool::from_observed(rows.seal(), candidate_input(99)).selection();
    assert_eq!(
        selection.select(CurrentOfferCandidateStatus::NotSelectable, 99),
        Some((OfferSelectPriority::CandidateUnique, 7))
    );
}
