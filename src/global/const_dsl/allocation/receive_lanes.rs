//! Assign lanes that preserve receive order and parallel ownership.
use super::lane_matching::{
    EndpointSet, LaneCompatibility, LaneEndpointIndex, maximum_lane_matching,
};
use super::{BYTE_DOMAIN, BYTE_DOMAIN_MASK_BYTES};
use crate::global::const_dsl::{
    receive_lane_causality::{FlowIndex, ReceiveScan, receive_lane_conflict_in},
    receive_pair_is_causal,
};
use crate::{
    g::SourceActorImage,
    global::const_dsl::{
        EffList, ScopeEvent, ScopeKind, route_arm_ranges_from_first_enter, scope::ScopeEntry,
    },
};

#[cfg(kani)]
mod kani;
#[cfg(all(test, hibana_repo_tests))]
mod tests;

// Candidate membership is compiler scratch. Every source parallel scope removes
// candidates that would collide with its opposite arm at the proposed color.
const fn direction_members<const E: usize>(
    events: &EffList<E>,
    actors: Option<&SourceActorImage<E>>,
    proposal: (crate::eff::EffAtom, u8),
) -> [bool; E] {
    let (before, color) = proposal;
    let mut selected = [false; E];
    let mut count = 0;
    let mut event = 0;
    while event < events.len() {
        let atom = events.atom_at(event);
        if atom.from == before.from && atom.to == before.to && atom.lane == before.lane {
            selected[event] = true;
            count += 1;
        }
        event += 1;
    }
    if count == 0 {
        return selected;
    }
    let markers = events.scope_markers();
    let mut marker = 0;
    while marker < markers.len() {
        let scope = markers.at(marker);
        if let ScopeEvent::Enter(ScopeEntry::Parallel { split }) = scope.event {
            let split = split as usize;
            let mut side = 0;
            while side < 2 {
                let (start, end, sibling_start, sibling_end) = if side == 0 {
                    (scope.offset(), split, split, scope.segment_end())
                } else {
                    (split, scope.segment_end(), scope.offset(), split)
                };
                let mut siblings = EndpointSet::<BYTE_DOMAIN_MASK_BYTES>::EMPTY;
                let mut sibling = sibling_start;
                while sibling < sibling_end {
                    if events.atom_at(sibling).lane == color {
                        siblings.include(events, actors, sibling);
                    }
                    sibling += 1;
                }
                event = start;
                while event < end {
                    if selected[event] {
                        let mut endpoints = EndpointSet::<BYTE_DOMAIN_MASK_BYTES>::EMPTY;
                        endpoints.include(events, actors, event);
                        if !endpoints.is_disjoint(siblings) {
                            selected[event] = false;
                        }
                    }
                    event += 1;
                }
                side += 1;
            }
        }
        marker += 1;
    }
    selected
}

const fn expand_direction<const E: usize>(
    events: &mut EffList<E>,
    scopes: &FlowIndex<E>,
    actors: Option<&SourceActorImage<E>>,
    proposal: (crate::eff::EffAtom, u8),
    #[cfg(hibana_profile)] counters: &mut crate::g::profiling::Counters,
) {
    let (before, color) = proposal;
    let mut selected = direction_members(events, actors, proposal);
    let mut outside = [0u16; E];
    let mut outside_len = 0;
    let mut candidate = scopes.first_receive_to(before.to);
    while candidate != u16::MAX {
        #[cfg(hibana_profile)]
        {
            counters.alignment_candidate_inspections += 1;
        }
        let other = events.atom_at(candidate as usize);
        if other.from != before.from && other.lane == color {
            outside[outside_len] = candidate;
            outside_len += 1;
        }
        candidate = scopes.next_receive_to(candidate);
    }
    let mut event = 0;
    while event < events.len() {
        if selected[event] {
            let mut index = 0;
            while index < outside_len {
                let candidate = outside[index] as usize;
                #[cfg(hibana_profile)]
                {
                    counters.alignment_pair_queries += 1;
                }
                let forward = receive_pair_is_causal(events, scopes, actors, event, candidate);
                let reverse = if forward {
                    #[cfg(hibana_profile)]
                    {
                        counters.alignment_pair_queries += 1;
                    }
                    receive_pair_is_causal(events, scopes, actors, candidate, event)
                } else {
                    false
                };
                if !forward || !reverse {
                    selected[event] = false;
                    break;
                }
                index += 1;
            }
        }
        event += 1;
    }
    // All selected events shared one old color and logical direction. They
    // introduce no conflicts with each other; each outside boundary was checked.
    event = 0;
    while event < events.len() {
        if selected[event] {
            let mut atom = events.atom_at(event);
            atom.lane = color;
            events.replace_atom(event, atom);
        }
        event += 1;
    }
}

const fn inspect<const E: usize>(
    events: &EffList<E>,
    scope_offsets: &FlowIndex<E>,
    actors: Option<&SourceActorImage<E>>,
    scan: ReceiveScan,
    #[cfg(hibana_profile)] counters: &mut crate::g::profiling::Counters,
) -> Result<(), (usize, usize, Option<u16>)> {
    #[cfg(hibana_profile)]
    if matches!(scan, ReceiveScan::From(Some(_))) {
        counters.resumed_inspections += 1;
    } else {
        counters.full_inspections += 1;
    }
    match receive_lane_conflict_in(events, scope_offsets, actors, scan) {
        Some(pair) => Err(pair),
        None => Ok(()),
    }
}

const fn align_range<const E: usize>(
    events: &mut EffList<E>,
    scope_offsets: &FlowIndex<E>,
    actors: Option<&SourceActorImage<E>>,
    arm: (usize, usize),
    conflict: (usize, usize),
    palette: (u16, u16),
    #[cfg(hibana_profile)] counters: &mut crate::g::profiling::Counters,
) -> bool {
    let (span, colors) = palette;
    #[cfg(hibana_profile)]
    {
        counters.alignment_attempts += 1;
    }
    let row_bytes = (colors as usize).div_ceil(8);
    let mut edges = [0u8; BYTE_DOMAIN * BYTE_DOMAIN_MASK_BYTES];
    let members =
        LaneEndpointIndex::<BYTE_DOMAIN_MASK_BYTES>::from_range(events, actors, arm.0, arm.1);
    let mut lane = 0;
    while lane < span {
        if members.contains_lane(lane as u8) {
            let mut color = 0;
            while color < colors {
                edges[lane as usize * row_bytes + color as usize / 8] |= 1 << (color % 8);
                color += 1;
            }
        }
        lane += 1;
    }
    // Only enclosing parallel scopes can have one side outside this arm.
    let mut scope = scope_offsets.enclosing_scope(arm.0);
    while scope != u16::MAX {
        if let Some((start, end)) = scope_offsets.parallel_sibling(scope, arm.0, arm.1) {
            let siblings =
                LaneEndpointIndex::<BYTE_DOMAIN_MASK_BYTES>::from_range(events, actors, start, end);
            lane = 0;
            while lane < span {
                if members.contains_lane(lane as u8) {
                    let mut color = 0;
                    while color < span {
                        if !members
                            .endpoint_set(lane as u8)
                            .is_disjoint(siblings.endpoint_set(color as u8))
                        {
                            edges[lane as usize * row_bytes + color as usize / 8] &=
                                !(1 << (color % 8));
                        }
                        color += 1;
                    }
                }
                lane += 1;
            }
        }
        scope = scope_offsets.parent_scope(scope);
    }
    let first_inside = arm.0 <= conflict.0 && conflict.0 < arm.1;
    let second_inside = arm.0 <= conflict.1 && conflict.1 < arm.1;
    if first_inside == second_inside {
        crate::invariant();
    }
    let (inside, outside) = if first_inside {
        (conflict.0, conflict.1)
    } else {
        (conflict.1, conflict.0)
    };
    let a = events.atom_at(inside);
    let b = events.atom_at(outside);
    edges[a.lane as usize * row_bytes + b.lane as usize / 8] &= !(1 << (b.lane % 8));
    // An injective map preserves constraints inside the range. Validate only
    // the cross-boundary pairs joined by a candidate map. A rejected pair
    // removes its edge permanently; the finite edge domain bounds refinement.
    let matching = loop {
        let matching = maximum_lane_matching::<0>(
            LaneCompatibility::Constraints {
                rows: edges.split_at(span as usize * row_bytes).0,
                row_bytes,
            },
            colors,
            span,
        );
        lane = 0;
        while lane < span {
            if members.contains_lane(lane as u8) && matching.left_for_right(lane).is_none() {
                return false;
            }
            lane += 1;
        }
        let mut rejected = false;
        let mut inside = arm.0;
        while inside < arm.1 && !rejected {
            let a = events.atom_at(inside);
            let color = match matching.left_for_right(a.lane as u16) {
                Some(color) => color as u8,
                None => crate::invariant(),
            };
            if a.from != a.to {
                let mut receiver_event = scope_offsets.first_receive_to(a.to);
                while receiver_event != u16::MAX {
                    let outside = receiver_event as usize;
                    #[cfg(hibana_profile)]
                    {
                        counters.alignment_candidate_inspections += 1;
                    }
                    if outside < arm.0 || arm.1 <= outside {
                        let b = events.atom_at(outside);
                        if b.from != b.to && a.to == b.to && a.from != b.from && color == b.lane {
                            #[cfg(hibana_profile)]
                            {
                                counters.alignment_pair_queries += 1;
                            }
                            let forward = receive_pair_is_causal(
                                events,
                                scope_offsets,
                                actors,
                                inside,
                                outside,
                            );
                            let reverse = if forward {
                                #[cfg(hibana_profile)]
                                {
                                    counters.alignment_pair_queries += 1;
                                }
                                receive_pair_is_causal(
                                    events,
                                    scope_offsets,
                                    actors,
                                    outside,
                                    inside,
                                )
                            } else {
                                false
                            };
                            if !forward || !reverse {
                                let edge =
                                    &mut edges[a.lane as usize * row_bytes + color as usize / 8];
                                let bit = 1 << (color % 8);
                                if *edge & bit == 0 {
                                    crate::invariant();
                                }
                                *edge &= !bit;
                                rejected = true;
                                break;
                            }
                        }
                    }
                    receiver_event = scope_offsets.next_receive_to(receiver_event);
                }
            }
            inside += 1;
        }
        if !rejected {
            break matching;
        }
    };
    let mut index = arm.0;
    while index < arm.1 {
        let mut atom = events.atom_at(index);
        atom.lane = match matching.left_for_right(atom.lane as u16) {
            Some(color) => color as u8,
            None => crate::invariant(),
        };
        events.replace_atom(index, atom);
        index += 1;
    }
    true
}

/// Solve source colorings against receive and parallel constraints. Prefer
/// the existing colors; use the wire domain when no such coloring exists.
/// All actor and causal facts come from this source.
/// The returned verdict is checked before any endpoint program is published.
pub(crate) const fn assign_receive_lanes<const E: usize>(
    events: &mut EffList<E>,
    actors: Option<&SourceActorImage<E>>,
    #[cfg(hibana_profile)] counters: &mut crate::g::profiling::Counters,
) -> bool {
    let scope_offsets = FlowIndex::new(events, actors);
    let mut checked = inspect(
        events,
        &scope_offsets,
        actors,
        ReceiveScan::From(None),
        #[cfg(hibana_profile)]
        counters,
    );
    if checked.is_ok() {
        return true;
    }
    let mut span = 0;
    let mut index = 0;
    while index < events.len() {
        let needed = events.atom_at(index).lane as u16 + 1;
        if span < needed {
            span = needed;
        }
        index += 1;
    }
    // Each successful recoloring separates a formerly conflicting source
    // pair and introduces none. The source event domain bounds this work.
    let mut remaining = match events.len().checked_mul(events.len()) {
        Some(value) => value,
        None => crate::invariant(),
    };
    while remaining != 0 {
        let (first, second, scope) = match checked {
            Ok(()) => crate::invariant(),
            Err(pair) => pair,
        };
        let mut changed = false;
        let mut palette = 0;
        while palette != 2 && !changed {
            let colors = if palette == 0 {
                span
            } else {
                BYTE_DOMAIN as u16
            };
            let before = events.atom_at(second);
            changed = align_range(
                events,
                &scope_offsets,
                actors,
                (second, second + 1),
                (first, second),
                (span, colors),
                #[cfg(hibana_profile)]
                counters,
            );
            if changed {
                let color = events.atom_at(second).lane;
                expand_direction(
                    events,
                    &scope_offsets,
                    actors,
                    (before, color),
                    #[cfg(hibana_profile)]
                    counters,
                );
            }
            let mut marker_index = events.scope_markers().len();
            while marker_index != 0 && !changed {
                marker_index -= 1;
                let markers = events.scope_markers();
                let marker = markers.at(marker_index);
                if marker.event.is_primary_enter()
                    && markers.is_first_enter(marker_index)
                    && matches!(marker.scope_id.kind(), Some(ScopeKind::Route))
                {
                    let arms = route_arm_ranges_from_first_enter(markers, marker_index);
                    let mut arm_index = 2;
                    while arm_index != 0 && !changed {
                        arm_index -= 1;
                        let (start, end) = arms[arm_index];
                        if (start <= first && first < end) != (start <= second && second < end) {
                            changed = align_range(
                                events,
                                &scope_offsets,
                                actors,
                                (start, end),
                                (first, second),
                                (span, colors),
                                #[cfg(hibana_profile)]
                                counters,
                            );
                        }
                    }
                }
            }
            palette += 1;
        }
        if !changed {
            return false;
        }
        #[cfg(hibana_profile)]
        {
            counters.recolorings += 1;
        }
        // Matching preserves the checked prefix: it is injective within the
        // arm and excludes incompatible pairs across its boundary. Resume at
        // the failed seed. If that suffix passes, recheck every earlier seed
        // in the final coloring. Each seed still checks its complete target
        // range, so the two scans cover the same obligations as a full scan.
        checked = inspect(
            events,
            &scope_offsets,
            actors,
            ReceiveScan::From(Some((first, scope))),
            #[cfg(hibana_profile)]
            counters,
        );
        if checked.is_ok() {
            return inspect(
                events,
                &scope_offsets,
                actors,
                ReceiveScan::Before((first, scope)),
                #[cfg(hibana_profile)]
                counters,
            )
            .is_ok();
        }
        index = 0;
        while index < events.len() {
            let needed = events.atom_at(index).lane as u16 + 1;
            if span < needed {
                span = needed;
            }
            index += 1;
        }
        remaining -= 1;
    }
    checked.is_ok()
}
