use super::*;
use crate::endpoint::kernel::frontier::{
    FrontierKind, OfferEntryAdmission, OfferEntryObservedState,
};
use crate::global::const_dsl::{ScopeId, ScopeKind};

#[test]
fn active_entry_set_accepts_the_complete_lane_domain() {
    let mut storage = [ActiveEntrySlot::EMPTY; 256];
    /* SAFETY: the builder exclusively owns the complete initialized test
    storage until it is sealed below. */
    let mut entries =
        unsafe { ActiveEntrySetBuilder::from_parts(storage.as_mut_ptr(), storage.len()) };

    for lane in 0u8..=u8::MAX {
        assert!(entries.insert_entry(lane as usize, lane));
    }

    assert_eq!(entries.len(), 256);
    assert!(!entries.insert_entry(256, u8::MAX));
    let entries = entries.seal();
    assert_eq!(entries.entry_at(8), Some(8));
    assert_eq!(entries.entry_at(255), Some(255));
}

#[test]
fn observed_entry_set_streams_beyond_the_former_eight_slot_mask() {
    let mut storage = [FrontierObservationSlot::EMPTY; 256];
    /* SAFETY: the builder exclusively owns the complete initialized test
    storage until it is sealed below. */
    let mut observed =
        unsafe { ObservedEntrySetBuilder::from_parts(storage.as_mut_ptr(), storage.len()) };
    observed.clear();

    for entry_idx in 0..256 {
        let (slot_idx, inserted) = observed
            .insert_entry(entry_idx)
            .expect("the descriptor-derived frontier must cover every active lane");
        assert!(inserted);
        assert_eq!(slot_idx, entry_idx);
        observed.record_observation(
            slot_idx,
            OfferEntryObservedState {
                scope_id: ScopeId::new(ScopeKind::Route, 1),
                frontier_mask: FrontierKind::Route.bit(),
                flags: OfferEntryObservedState::FLAG_READY,
            },
            FrontierKind::Route.bit(),
            OfferEntryAdmission::Selectable,
        );
    }

    let observed = observed.seal();
    assert_eq!(observed.len(), 256);
    assert_eq!(observed.entry_idx(8), Some(8));
    assert_eq!(observed.entry_idx(255), Some(255));
    assert!(observed.slot(255).is_some_and(|slot| {
        slot.is_selectable() && slot.is_ready() && slot.is_in_frontier(FrontierKind::Route)
    }));
}
