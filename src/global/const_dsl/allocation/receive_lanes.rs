//! Assign lanes that preserve receive order and parallel ownership.
use super::lane_matching::{LaneCompatibility, LaneEndpointIndex, maximum_lane_matching};
use super::{BYTE_DOMAIN, BYTE_DOMAIN_MASK_BYTES};
use crate::global::const_dsl::{
    receive_lane_causality::{
        FlowIndex, LogicalDirections, Participants, ReceiveOrder, ReceiveRelations, ReceiveScan,
        ReceiveSeeds, has_receive_sender_changes, receive_lane_conflict_in,
    },
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
    scopes: &FlowIndex<E>,
    directions: &LogicalDirections<E>,
    proposal: (usize, u8, u8),
    colored: &mut Participants<E>,
) -> [bool; E] {
    let (source_event, old_lane, color) = proposal;
    let mut selected = [false; E];
    let mut count = 0;
    let mut selected_start = events.len();
    let mut selected_end = 0;
    let head = directions.first(source_event);
    let mut position = head;
    while position != u16::MAX {
        let event = position as usize;
        if events.atom_at(event).lane == old_lane {
            selected[event] = true;
            count += 1;
            if event < selected_start {
                selected_start = event;
            }
            selected_end = event + 1;
        }
        position = directions.next(position);
    }
    if count == 0 {
        return selected;
    }
    scopes.lane_participants(events, color, colored);
    let markers = events.scope_markers();
    let mut marker = 0;
    while marker < markers.len() {
        let scope = markers.at(marker);
        if let ScopeEvent::Enter(ScopeEntry::Parallel { split }) = scope.event {
            if scope.segment_end() <= selected_start || selected_end <= scope.offset() {
                marker += 1;
                continue;
            }
            let split = split as usize;
            let mut side = 0;
            while side < 2 {
                let (start, end, sibling_start, sibling_end) = if side == 0 {
                    (scope.offset(), split, split, scope.segment_end())
                } else {
                    (split, scope.segment_end(), scope.offset(), split)
                };
                let mut first = head;
                while first != u16::MAX && ((first as usize) < start || !selected[first as usize]) {
                    first = directions.next(first);
                }
                if first == u16::MAX || first as usize >= end {
                    side += 1;
                    continue;
                }
                let siblings = colored.members(sibling_start, sibling_end);
                position = first;
                while position != u16::MAX && (position as usize) < end {
                    let event = position as usize;
                    if selected[event] {
                        let endpoints = scopes.participants_at(event);
                        if !endpoints.is_disjoint(siblings) {
                            selected[event] = false;
                            count -= 1;
                            if count == 0 {
                                return selected;
                            }
                        }
                    }
                    position = directions.next(position);
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
    directions: &LogicalDirections<E>,
    proposal: (usize, u8, u8),
    closures: Option<ReceiveOrder<'_>>,
    colored: &mut Participants<E>,
    #[cfg(hibana_profile)] counters: &mut crate::g::profiling::Counters,
) {
    let (source_event, _, color) = proposal;
    let before = events.atom_at(source_event);
    let head = directions.first(source_event);
    let mut selected = direction_members(events, scopes, directions, proposal, colored);
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
    let mut position = head;
    while position != u16::MAX {
        let event = position as usize;
        if selected[event] {
            let mut index = 0;
            while index < outside_len {
                let candidate = outside[index] as usize;
                #[cfg(hibana_profile)]
                {
                    counters.alignment_pair_queries += 1;
                }
                let forward =
                    receive_pair_is_causal(events, scopes, actors, event, candidate, closures);
                let reverse = if forward {
                    #[cfg(hibana_profile)]
                    {
                        counters.alignment_pair_queries += 1;
                    }
                    receive_pair_is_causal(events, scopes, actors, candidate, event, closures)
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
        position = directions.next(position);
    }
    // All selected events shared one old color and logical direction. They
    // introduce no conflicts with each other; each outside boundary was checked.
    position = head;
    while position != u16::MAX {
        let event = position as usize;
        if selected[event] {
            let mut atom = events.atom_at(event);
            atom.lane = color;
            events.replace_atom(event, atom);
        }
        position = directions.next(position);
    }
}

const fn inspect<const E: usize>(
    events: &EffList<E>,
    scope_offsets: &FlowIndex<E>,
    actors: Option<&SourceActorImage<E>>,
    scan: ReceiveScan,
    closures: Option<ReceiveOrder<'_>>,
    #[cfg(hibana_profile)] counters: &mut crate::g::profiling::Counters,
) -> Result<(), (usize, usize, Option<u16>)> {
    #[cfg(hibana_profile)]
    if matches!(scan, ReceiveScan::From(Some(_))) {
        counters.resumed_inspections += 1;
    } else {
        counters.full_inspections += 1;
    }
    match receive_lane_conflict_in(events, scope_offsets, actors, scan, closures) {
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
    closures: Option<ReceiveOrder<'_>>,
    #[cfg(hibana_profile)] counters: &mut crate::g::profiling::Counters,
) -> Option<u16> {
    let (span, colors) = palette;
    #[cfg(hibana_profile)]
    {
        counters.alignment_attempts += 1;
    }
    let row_bytes = (colors as usize).div_ceil(8);
    let mut edges = [0u8; BYTE_DOMAIN * BYTE_DOMAIN_MASK_BYTES];
    let members =
        LaneEndpointIndex::<BYTE_DOMAIN_MASK_BYTES>::from_range(events, actors, arm.0, arm.1);
    let mut vertices = [u16::MAX; BYTE_DOMAIN];
    let mut vertex_count = 0;
    let mut lane = 0;
    while lane < span {
        if members.contains_lane(lane as u8) {
            let vertex = vertex_count;
            vertices[lane as usize] = vertex;
            vertex_count += 1;
            let mut color = 0;
            while color < colors {
                edges[vertex as usize * row_bytes + color as usize / 8] |= 1 << (color % 8);
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
                            edges[vertices[lane as usize] as usize * row_bytes
                                + color as usize / 8] &= !(1 << (color % 8));
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
    edges[vertices[a.lane as usize] as usize * row_bytes + b.lane as usize / 8] &=
        !(1 << (b.lane % 8));
    // An injective map preserves constraints inside the range. Validate only
    // the cross-boundary pairs joined by a candidate map. A rejected pair
    // removes its edge permanently; the finite edge domain bounds refinement.
    // A row denotes one old lane class, not one event. Once all events of
    // that class pass against an outside color, that edge stays valid while
    // other matching edges are refined. Source lanes change only at commit.
    let mut confirmed = [0u8; BYTE_DOMAIN * BYTE_DOMAIN_MASK_BYTES];
    let matching = loop {
        let matching = maximum_lane_matching::<0>(
            LaneCompatibility::Constraints {
                rows: edges.split_at(vertex_count as usize * row_bytes).0,
                row_bytes,
            },
            colors,
            vertex_count,
        );
        lane = 0;
        while lane < vertex_count {
            if matching.left_for_right(lane).is_none() {
                return None;
            }
            lane += 1;
        }
        let mut rejected = false;
        lane = 0;
        while lane < span && !rejected {
            let vertex = vertices[lane as usize];
            if vertex == u16::MAX {
                lane += 1;
                continue;
            }
            let color = match matching.left_for_right(vertex) {
                Some(color) => color as u8,
                None => crate::invariant(),
            };
            let edge_index = vertex as usize * row_bytes + color as usize / 8;
            let bit = 1 << (color % 8);
            if confirmed[edge_index] & bit != 0 {
                lane += 1;
                continue;
            }
            let mut inside = arm.0;
            while inside < arm.1 && !rejected {
                let a = events.atom_at(inside);
                if a.lane as u16 != lane || a.from == a.to {
                    inside += 1;
                    continue;
                }
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
                                closures,
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
                                    closures,
                                )
                            } else {
                                false
                            };
                            if !forward || !reverse {
                                let edge = &mut edges[edge_index];
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
                inside += 1;
            }
            if !rejected {
                confirmed[edge_index] |= bit;
            }
            lane += 1;
        }
        if !rejected {
            break matching;
        }
    };
    let mut assigned_span = 0;
    let mut index = arm.0;
    while index < arm.1 {
        let mut atom = events.atom_at(index);
        atom.lane = match matching.left_for_right(vertices[atom.lane as usize]) {
            Some(color) => color as u8,
            None => crate::invariant(),
        };
        let needed = atom.lane as u16 + 1;
        if assigned_span < needed {
            assigned_span = needed;
        }
        events.replace_atom(index, atom);
        index += 1;
    }
    Some(assigned_span)
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
    if !has_receive_sender_changes(events) {
        return true;
    }
    let scope_offsets = FlowIndex::new(events, actors);
    let fixed = match actors {
        None => true,
        Some(image) => image.decision_count() == 0,
    };
    if fixed {
        let seeds = ReceiveSeeds::new(events);
        let count = seeds.count;
        macro_rules! prepare {
            ($words:literal) => {
                assign_fixed::<E, $words>(
                    events,
                    (&scope_offsets, actors, &seeds),
                    #[cfg(hibana_profile)]
                    counters,
                )
            };
        }
        if count <= 64 {
            prepare!(1)
        } else if count <= 128 {
            prepare!(2)
        } else if count <= 256 {
            prepare!(4)
        } else if count <= 512 {
            prepare!(8)
        } else if count <= 1024 {
            prepare!(16)
        } else if count <= 2048 {
            prepare!(32)
        } else if count <= 4096 {
            prepare!(64)
        } else if count <= 8192 {
            prepare!(128)
        } else if count <= 16384 {
            prepare!(256)
        } else if count <= 32768 {
            prepare!(512)
        } else {
            prepare!(1024)
        }
    } else {
        let checked = inspect(
            events,
            &scope_offsets,
            actors,
            ReceiveScan::From(None),
            None,
            #[cfg(hibana_profile)]
            counters,
        );
        if checked.is_ok() {
            return true;
        }
        assign_prepared(
            events,
            (&scope_offsets, actors, None),
            checked,
            #[cfg(hibana_profile)]
            counters,
        )
    }
}

const fn assign_fixed<const E: usize, const W: usize>(
    events: &mut EffList<E>,
    source: (
        &FlowIndex<E>,
        Option<&SourceActorImage<E>>,
        &ReceiveSeeds<E>,
    ),
    #[cfg(hibana_profile)] counters: &mut crate::g::profiling::Counters,
) -> bool {
    let (scopes, actors, seeds) = source;
    let order = ReceiveRelations::<E, W>::new(events, scopes, actors, seeds);
    let closures = Some(order.as_ref());
    let checked = inspect(
        events,
        scopes,
        actors,
        ReceiveScan::From(None),
        closures,
        #[cfg(hibana_profile)]
        counters,
    );
    if checked.is_ok() {
        return true;
    }
    assign_prepared(
        events,
        (scopes, actors, closures),
        checked,
        #[cfg(hibana_profile)]
        counters,
    )
}

const fn assign_prepared<const E: usize>(
    events: &mut EffList<E>,
    source: (
        &FlowIndex<E>,
        Option<&SourceActorImage<E>>,
        Option<ReceiveOrder<'_>>,
    ),
    mut checked: Result<(), (usize, usize, Option<u16>)>,
    #[cfg(hibana_profile)] counters: &mut crate::g::profiling::Counters,
) -> bool {
    let (scope_offsets, actors, closures) = source;
    let directions = LogicalDirections::new(events);
    let mut colored = Participants::empty(events.len());
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
            let assigned = align_range(
                events,
                scope_offsets,
                actors,
                (second, second + 1),
                (first, second),
                (span, colors),
                closures,
                #[cfg(hibana_profile)]
                counters,
            );
            if let Some(assigned_span) = assigned {
                changed = true;
                if span < assigned_span {
                    span = assigned_span;
                }
                let color = events.atom_at(second).lane;
                expand_direction(
                    events,
                    scope_offsets,
                    actors,
                    &directions,
                    (second, before.lane, color),
                    closures,
                    &mut colored,
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
                            let assigned = align_range(
                                events,
                                scope_offsets,
                                actors,
                                (start, end),
                                (first, second),
                                (span, colors),
                                closures,
                                #[cfg(hibana_profile)]
                                counters,
                            );
                            if let Some(assigned_span) = assigned {
                                changed = true;
                                if span < assigned_span {
                                    span = assigned_span;
                                }
                            }
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
            scope_offsets,
            actors,
            ReceiveScan::From(Some((first, scope))),
            closures,
            #[cfg(hibana_profile)]
            counters,
        );
        if checked.is_ok() {
            return inspect(
                events,
                scope_offsets,
                actors,
                ReceiveScan::Before((first, scope)),
                closures,
                #[cfg(hibana_profile)]
                counters,
            )
            .is_ok();
        }
        remaining -= 1;
    }
    checked.is_ok()
}
