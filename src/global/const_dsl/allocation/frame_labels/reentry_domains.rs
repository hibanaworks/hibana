use super::{BYTE_DOMAIN_MASK_BYTES, first_available, insert};
use crate::global::const_dsl::{EffList, ScopeKind};

#[cfg(all(test, hibana_repo_tests))]
mod tests;

/// Preserve route-path separation and distinguish independent elastic owners.
/// A completed roll can reenter while a different continuation is eligible.
/// Sequential ordering alone therefore cannot justify sharing its wire color.
pub(crate) const fn separate_roll_frame_domains<const E: usize>(source: &mut EffList<E>) {
    let markers = source.scope_markers();
    let mut has_roll = false;
    let mut marker_idx = 0usize;
    while marker_idx < markers.len() {
        if markers.at(marker_idx).event.is_roll_enter() {
            has_roll = true;
            break;
        }
        marker_idx += 1;
    }
    // Ordinary sequential/parallel programs need no elastic owner refinement.
    // Avoid even constructing per-event scratch or scanning it in this case.
    if !has_roll {
        return;
    }
    let mut original = [0u8; E];
    let mut owners = [0u16; E];
    let mut event = 0usize;
    while event < source.len() {
        original[event] = source.frame_label_at(event);
        let mut narrowest = usize::MAX;
        let mut marker_idx = 0usize;
        while marker_idx < markers.len() {
            let marker = markers.at(marker_idx);
            if marker.event.is_roll_enter()
                && matches!(marker.scope_id.kind(), Some(ScopeKind::Roll))
                && marker.offset() <= event
                && event < marker.segment_end()
            {
                let span = marker.segment_end() - marker.offset();
                // SourceLowering assigns unique preorder ordinals. In its
                // laminar intervals, smallest span selects the innermost roll;
                // the later ordinal selects the inner coextensive wrapper.
                let owner = marker.scope_id.local_ordinal() + 1;
                if span < narrowest || (span == narrowest && owner > owners[event]) {
                    narrowest = span;
                    owners[event] = owner;
                }
            }
            marker_idx += 1;
        }
        event += 1;
    }
    event = 0;
    while event < source.len() {
        let current = source.atom_at(event);
        if current.from != current.to {
            let mut used = [0u8; BYTE_DOMAIN_MASK_BYTES];
            let mut prior = 0usize;
            while prior < event {
                let previous = source.atom_at(prior);
                if previous.from == current.from
                    && previous.to == current.to
                    && previous.lane == current.lane
                    && (original[prior] != original[event] || owners[prior] != owners[event])
                {
                    insert(&mut used, source.frame_label_at(prior));
                }
                prior += 1;
            }
            let Some(color) = first_available(&used) else {
                panic!("elastic roll frame domains exceed wire color capacity");
            };
            source.set_frame_label(event, color);
        }
        event += 1;
    }
}
