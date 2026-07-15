use super::super::{color_roll_frame_labels, merge_route_frame_labels};
use super::atom;
use crate::global::const_dsl::{EffList, ReentryMark, ScopeId, ScopeKind};

#[kani::proof]
#[kani::unwind(160)]
fn nested_roll_frame_coloring_uses_the_complete_inbound_key() {
    let same_source: bool = kani::any();
    let prefix_from = 0;
    let nested_from = if same_source { 0 } else { 1 };

    let mut source = EffList::<14>::new_partitioned(4, 10, 0);
    let roll = ScopeId::new(ScopeKind::Roll, 0);
    let outer = ScopeId::new(ScopeKind::Route, 1);
    let inner = ScopeId::new(ScopeKind::Route, 2);
    source.push_scope_enter_reentry_mut(0, roll, ReentryMark::SinglePass);
    source.push_scope_enter_reentry_mut(0, outer, ReentryMark::Reentrant);
    source.push_event_mut(atom(2, 3, 0));
    source.close_scope_segment_mut(outer, 0, 1);
    source.push_scope_exit_mut(1, outer);
    source.push_scope_enter_reentry_mut(1, outer, ReentryMark::Reentrant);
    source.push_event_mut(atom(prefix_from, 3, 0));
    source.push_scope_enter_reentry_mut(2, inner, ReentryMark::Reentrant);
    source.push_event_mut(atom(nested_from, 3, 0));
    source.close_scope_segment_mut(inner, 2, 3);
    source.push_scope_exit_mut(3, inner);
    source.push_scope_enter_reentry_mut(3, inner, ReentryMark::Reentrant);
    source.push_event_mut(atom(nested_from, 3, 0));
    source.close_scope_segment_mut(inner, 3, 4);
    source.push_scope_exit_mut(4, inner);
    merge_route_frame_labels(&mut source, 2, 3, 4);
    source.close_scope_segment_mut(outer, 1, 4);
    source.push_scope_exit_mut(4, outer);
    merge_route_frame_labels(&mut source, 0, 1, 4);
    source.close_scope_segment_mut(roll, 0, 4);
    source.push_scope_exit_mut(4, roll);
    color_roll_frame_labels(&mut source, 0, 4);

    assert!(source.frame_label_at(1) == 0);
    assert!(source.frame_label_at(2) == if same_source { 1 } else { 0 });
}
