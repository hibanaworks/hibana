use super::BYTE_DOMAIN;
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
    let mut selectors = [0u32; E];
    let mut ordered = [0u16; E];
    let mut count = 0usize;
    let mut event = 0usize;
    while event < source.len() {
        original[event] = source.frame_label_at(event);
        let atom = source.atom_at(event);
        selectors[event] = ((atom.from as u32) << 16) | ((atom.to as u32) << 8) | atom.lane as u32;
        if atom.from != atom.to {
            ordered[count] = event as u16;
            count += 1;
        }
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
    // Stable byte-wise sorting groups each logical direction and lane while
    // preserving source order within the group. Read each source atom once.
    let mut scratch = [0u16; E];
    let mut shift = 0;
    while shift < 24 {
        let mut offsets = [0usize; BYTE_DOMAIN];
        let mut index = 0;
        while index < count {
            let key = ((selectors[ordered[index] as usize] >> shift) & 255) as usize;
            offsets[key] += 1;
            index += 1;
        }
        let mut next = 0;
        let mut key = 0;
        while key < BYTE_DOMAIN {
            let size = offsets[key];
            offsets[key] = next;
            next += size;
            key += 1;
        }
        index = 0;
        while index < count {
            let event = ordered[index];
            let key = ((selectors[event as usize] >> shift) & 255) as usize;
            scratch[offsets[key]] = event;
            offsets[key] += 1;
            index += 1;
        }
        index = 0;
        while index < count {
            ordered[index] = scratch[index];
            index += 1;
        }
        shift += 8;
    }
    // One color per (original color, innermost roll) class. There can be at
    // most 256 classes for a selector, independent of the source event count.
    let mut start = 0;
    while start < count {
        let selector = selectors[ordered[start] as usize];
        let mut representatives = [0u16; BYTE_DOMAIN];
        let mut classes = 0;
        let mut index = start;
        while index < count && selectors[ordered[index] as usize] == selector {
            let event = ordered[index] as usize;
            let mut color = 0;
            while color < classes {
                let prior = representatives[color] as usize;
                if original[prior] == original[event] && owners[prior] == owners[event] {
                    break;
                }
                color += 1;
            }
            if color == classes {
                if classes == BYTE_DOMAIN {
                    panic!("elastic roll frame domains exceed wire color capacity");
                }
                representatives[classes] = event as u16;
                classes += 1;
            }
            source.set_frame_label(event, color as u8);
            index += 1;
        }
        start = index;
    }
}
