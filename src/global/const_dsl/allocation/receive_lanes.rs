//! Assign lanes that preserve receive order and parallel ownership.
use super::lane_matching::{LaneCompatibility, maximum_lane_matching};
use super::{BYTE_DOMAIN, BYTE_DOMAIN_MASK_BYTES};
use crate::global::const_dsl::{
    receive_lane_causality::{
        CausalRoles, Direction, FlowIndex, LogicalDirections, ReceiveDirectionIndex, ReceiveLanes,
        ReceiveOrder, ReceiveRelations, ReceiveScan, ReceiveSeeds, passive_choice_conflict,
        receive_lane_conflict_in, receive_lane_conflict_indexed,
    },
    receive_pair_is_causal,
};
use crate::{
    g::SourceActorImage,
    global::const_dsl::{EffList, ScopeKind, route_arm_ranges_from_first_enter},
};

mod coloring;
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
) -> [bool; E] {
    let (source_event, old_lane, color) = proposal;
    let mut selected = [false; E];
    let mut position = directions.first(source_event);
    while position != u16::MAX {
        let event = position as usize;
        if events.atom_at(event).lane == old_lane {
            let endpoints = scopes.participants_at(event);
            let mut eligible = true;
            // Only ancestors can place this event opposite another parallel arm.
            // The source index already contains their complete nesting chain.
            let mut scope = scopes.enclosing_scope(event);
            while scope != u16::MAX {
                if let Some((start, end)) = scopes.parallel_sibling(scope, event, event + 1) {
                    let mut colors = [0; BYTE_DOMAIN_MASK_BYTES];
                    colors[color as usize / 8] = 1 << (color % 8);
                    scopes.exclude_lane_colors(events, (start, end, endpoints), &mut colors);
                    if colors[color as usize / 8] == 0 {
                        eligible = false;
                        break;
                    }
                }
                scope = scopes.parent_scope(scope);
            }
            selected[event] = eligible;
        }
        position = directions.next(position);
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
    changed_receivers: &mut CausalRoles,
    #[cfg(hibana_profile)] counters: &mut crate::g::profiling::Counters,
) {
    let (source_event, _, color) = proposal;
    let before = events.atom_at(source_event);
    let head = directions.first(source_event);
    let mut selected = direction_members(events, scopes, directions, proposal);
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
            if atom.from != atom.to {
                changed_receivers.insert(atom.to);
            }
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
    directions: &ReceiveDirectionIndex<'_>,
    #[cfg(hibana_profile)] counters: &mut crate::g::profiling::Counters,
) -> Result<(), (usize, usize, Option<u16>)> {
    #[cfg(hibana_profile)]
    if matches!(scan, ReceiveScan::From(Some(_))) {
        counters.resumed_inspections += 1;
    } else {
        counters.full_inspections += 1;
    }
    match receive_lane_conflict_indexed(events, scope_offsets, actors, scan, closures, directions) {
        Some(pair) => Err(pair),
        None => Ok(()),
    }
}

const fn align_range<const E: usize>(
    events: &mut EffList<E>,
    indexes: (
        &FlowIndex<E>,
        &ReceiveLanes<E>,
        Option<&SourceActorImage<E>>,
    ),
    arm: (usize, usize),
    conflict: (usize, usize),
    palette: (u16, u16),
    closures: Option<ReceiveOrder<'_>>,
    changed_receivers: &mut CausalRoles,
    #[cfg(hibana_profile)] counters: &mut crate::g::profiling::Counters,
) -> Option<u16> {
    let (scope_offsets, receives, actors) = indexes;
    let (span, colors) = palette;
    #[cfg(hibana_profile)]
    {
        counters.alignment_attempts += 1;
    }
    let row_bytes = (colors as usize).div_ceil(8);
    let mut edges = [0u8; BYTE_DOMAIN * BYTE_DOMAIN_MASK_BYTES];
    let mut members = [CausalRoles::empty(); BYTE_DOMAIN];
    let mut event = arm.0;
    while event < arm.1 {
        let lane = events.atom_at(event).lane as usize;
        members[lane] = members[lane].union(scope_offsets.participants_at(event));
        event += 1;
    }
    let arm_participants = scope_offsets.participants_in(arm.0, arm.1);
    let mut vertices = [u16::MAX; BYTE_DOMAIN];
    let mut vertex_count = 0;
    let mut lane = 0;
    while lane < span {
        if !members[lane as usize].is_empty() {
            let vertex = vertex_count;
            vertices[lane as usize] = vertex;
            vertex_count += 1;
            let mut byte = 0;
            while byte < row_bytes {
                edges[vertex as usize * row_bytes + byte] =
                    if byte + 1 == row_bytes && colors % 8 != 0 {
                        ((1u16 << (colors % 8)) - 1) as u8
                    } else {
                        u8::MAX
                    };
                byte += 1;
            }
        }
        lane += 1;
    }
    // Only enclosing parallel scopes can have one side outside this arm.
    let mut scope = scope_offsets.enclosing_scope(arm.0);
    while scope != u16::MAX {
        if let Some((start, end)) = scope_offsets.parallel_sibling(scope, arm.0, arm.1)
            && !arm_participants.is_disjoint(scope_offsets.participants_in(start, end))
        {
            lane = 0;
            while lane < span {
                if !members[lane as usize].is_empty() {
                    let offset = vertices[lane as usize] as usize * row_bytes;
                    let row = edges.split_at_mut(offset).1.split_at_mut(row_bytes).0;
                    scope_offsets.exclude_lane_colors(
                        events,
                        (start, end, members[lane as usize]),
                        row,
                    );
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
                let mut receiver_event = receives.first(a.to, color);
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
                    receiver_event = receives.next(receiver_event);
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
        if atom.from != atom.to {
            changed_receivers.insert(atom.to);
        }
        index += 1;
    }
    Some(assigned_span)
}

/// Solve source colorings against receive and parallel constraints. Prefer
/// the existing colors; use the wire domain when no such coloring exists.
/// All actor and causal facts come from this source.
/// The returned verdict is checked before any endpoint program is published.
const fn assign_ordered_receive_lanes<const E: usize>(
    events: &mut EffList<E>,
    actors: Option<&SourceActorImage<E>>,
    #[cfg(hibana_profile)] counters: &mut crate::g::profiling::Counters,
) -> bool {
    let mut direction_storage = [Direction::EMPTY; E];
    let mut direction_storage_homogeneous = [false; E];
    #[cfg(hibana_profile)]
    {
        counters.receive_index_builds += 1;
    }
    let mut receive_directions = ReceiveDirectionIndex::new(
        events,
        &mut direction_storage,
        &mut direction_storage_homogeneous,
    );
    if !receive_directions.has_changes() {
        return true;
    }
    if coloring::refine(
        events,
        #[cfg(hibana_profile)]
        counters,
    ) {
        #[cfg(hibana_profile)]
        {
            counters.recolorings += 1;
            counters.receive_index_builds += 1;
        }
        let checked = ReceiveDirectionIndex::new(
            events,
            &mut direction_storage,
            &mut direction_storage_homogeneous,
        );
        return !checked.has_changes();
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
                    &mut receive_directions,
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
            &receive_directions,
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
            &mut receive_directions,
            #[cfg(hibana_profile)]
            counters,
        )
    }
}

/// Refine only after the ordinary receive-order map is valid. A compatible
/// same-direction merge can retain an existing FIFO fence across visits.
/// Every candidate is checked in compiler scratch and restored on failure;
/// no intermediate map is published. Strict conflict order bounds the search.
pub(crate) const fn assign_receive_lanes<const E: usize>(
    events: &mut EffList<E>,
    actors: Option<&SourceActorImage<E>>,
    #[cfg(hibana_profile)] counters: &mut crate::g::profiling::Counters,
) -> bool {
    if !assign_ordered_receive_lanes(
        events,
        actors,
        #[cfg(hibana_profile)]
        counters,
    ) {
        return false;
    }
    let Some(mut conflict) = passive_choice_conflict(events, actors, None) else {
        return true;
    };
    let scopes = FlowIndex::new(events, actors);
    let directions = LogicalDirections::new(events);
    loop {
        let entry = conflict.1;
        let target = events.atom_at(entry);
        let mut candidate = directions.first(entry);
        let mut repaired = false;
        while candidate != u16::MAX {
            let color = events.atom_at(candidate as usize).lane;
            if color != target.lane {
                let members =
                    direction_members(events, &scopes, &directions, (entry, target.lane, color));
                if members[entry] {
                    let mut event = 0;
                    while event < events.len() {
                        if members[event] {
                            let mut atom = events.atom_at(event);
                            atom.lane = color;
                            events.replace_atom(event, atom);
                        }
                        event += 1;
                    }
                    let ordered = receive_lane_conflict_in(
                        events,
                        &scopes,
                        actors,
                        ReceiveScan::From(None),
                        None,
                    )
                    .is_none();
                    if ordered {
                        let next = passive_choice_conflict(events, actors, Some(&scopes));
                        if let Some(next) = next {
                            if passive_conflict_advances(events, conflict, next) {
                                conflict = next;
                                repaired = true;
                            }
                        } else {
                            #[cfg(hibana_profile)]
                            {
                                counters.recolorings += 1;
                            }
                            return true;
                        }
                    }
                    if repaired {
                        break;
                    }
                    let mut event = 0;
                    while event < events.len() {
                        if members[event] {
                            let mut atom = events.atom_at(event);
                            atom.lane = target.lane;
                            events.replace_atom(event, atom);
                        }
                        event += 1;
                    }
                }
            }
            candidate = directions.next(candidate);
        }
        if !repaired {
            return false;
        }
        #[cfg(hibana_profile)]
        {
            counters.recolorings += 1;
        }
    }
}

const fn passive_conflict_advances<const E: usize>(
    events: &EffList<E>,
    before: (usize, usize, Option<u16>),
    after: (usize, usize, Option<u16>),
) -> bool {
    let markers = events.scope_markers();
    let mut first = usize::MAX;
    let mut second = usize::MAX;
    let mut index = 0;
    while index < markers.len() {
        let marker = markers.at(index);
        if marker.event.is_primary_enter() {
            let ordinal = marker.scope_id.local_ordinal();
            if matches!(before.2, Some(scope) if scope == ordinal) {
                first = index;
            }
            if matches!(after.2, Some(scope) if scope == ordinal) {
                second = index;
            }
        }
        index += 1;
    }
    if first == usize::MAX || second == usize::MAX {
        crate::invariant();
    }
    first < second
        || first == second && (before.0 < after.0 || before.0 == after.0 && before.1 < after.1)
}

const fn assign_fixed<const E: usize, const W: usize>(
    events: &mut EffList<E>,
    source: (
        &FlowIndex<E>,
        Option<&SourceActorImage<E>>,
        &ReceiveSeeds<E>,
    ),
    receive_directions: &mut ReceiveDirectionIndex<'_>,
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
        receive_directions,
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
        receive_directions,
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
    receive_directions: &mut ReceiveDirectionIndex<'_>,
    #[cfg(hibana_profile)] counters: &mut crate::g::profiling::Counters,
) -> bool {
    let (scope_offsets, actors, closures) = source;
    let directions = LogicalDirections::new(events);
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
        let receives = ReceiveLanes::new(events);
        let mut changed_receivers = CausalRoles::empty();
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
                (scope_offsets, &receives, actors),
                (second, second + 1),
                (first, second),
                (span, colors),
                closures,
                &mut changed_receivers,
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
                    &mut changed_receivers,
                    #[cfg(hibana_profile)]
                    counters,
                );
            }
            // A route separating this pair must enclose one of its events.
            // Merge the two ancestor chains in the same descending source
            // order as the former marker traversal, without visiting siblings.
            let mut left_scope = scope_offsets.enclosing_scope(first);
            let mut right_scope = scope_offsets.enclosing_scope(second);
            while (left_scope != u16::MAX || right_scope != u16::MAX) && !changed {
                let marker_index = if left_scope == u16::MAX {
                    right_scope
                } else if right_scope == u16::MAX || left_scope > right_scope {
                    left_scope
                } else {
                    right_scope
                };
                if left_scope == marker_index {
                    left_scope = scope_offsets.parent_scope(left_scope);
                }
                if right_scope == marker_index {
                    right_scope = scope_offsets.parent_scope(right_scope);
                }
                let marker_index = marker_index as usize;
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
                                (scope_offsets, &receives, actors),
                                (start, end),
                                (first, second),
                                (span, colors),
                                closures,
                                &mut changed_receivers,
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
        receive_directions.refresh(events, scope_offsets, changed_receivers);
        checked = inspect(
            events,
            scope_offsets,
            actors,
            ReceiveScan::From(Some((first, scope))),
            closures,
            receive_directions,
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
                receive_directions,
                #[cfg(hibana_profile)]
                counters,
            )
            .is_ok();
        }
        remaining -= 1;
    }
    checked.is_ok()
}
