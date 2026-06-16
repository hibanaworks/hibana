use super::{FrameLabelMask, ScopeFrameLabelMeta, ScopeFrameLabelScratch, ScopeFrameLabelView};

#[test]
fn overlapping_frame_label_is_not_route_evidence() {
    let mut scratch = ScopeFrameLabelScratch::EMPTY;
    scratch.record_arm_frame_label(0, 7);
    scratch.record_arm_frame_label(1, 7);
    let meta = scratch.view();

    assert_eq!(meta.evidence_arm_for_frame_label(7), None);
    assert!(!meta.frame_hint_mask().contains_frame_label(7));
}

#[test]
fn unique_frame_label_remains_route_evidence() {
    let mut scratch = ScopeFrameLabelScratch::EMPTY;
    scratch.record_arm_frame_label(0, 7);
    scratch.record_arm_frame_label(1, 8);
    let meta = scratch.view();

    assert_eq!(meta.evidence_arm_for_frame_label(7), Some(0));
    assert_eq!(meta.evidence_arm_for_frame_label(8), Some(1));
    assert!(meta.frame_hint_mask().contains_frame_label(7));
    assert!(meta.frame_hint_mask().contains_frame_label(8));
    assert!(!FrameLabelMask::EMPTY.contains_frame_label(7));
}

#[test]
fn controller_frame_label_exclusion_does_not_need_duplicate_masks() {
    let mut scratch = ScopeFrameLabelScratch::EMPTY;
    scratch.meta_mut().controller_frame_labels[0] = 7;
    scratch.meta_mut().flags |= ScopeFrameLabelMeta::FLAG_CONTROLLER_ARM0;
    scratch.record_arm_frame_label(0, 7);
    scratch.exclude_controller_arm_frame_label_from_evidence(0, 7);
    let meta = scratch.view();

    assert_eq!(meta.evidence_arm_for_frame_label(7), None);
    assert!(meta.frame_hint_mask().contains_frame_label(7));
}

#[test]
fn scope_frame_label_meta_size_budget() {
    assert_eq!(core::mem::size_of::<ScopeFrameLabelMeta>(), 5);
    assert_eq!(core::mem::align_of::<ScopeFrameLabelMeta>(), 1);
    assert!(core::mem::size_of::<ScopeFrameLabelView<'_>>() <= 16);
    assert!(core::mem::size_of::<ScopeFrameLabelScratch>() <= 72);
}
