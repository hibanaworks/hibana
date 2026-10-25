//! Assign lanes that preserve receive order and parallel ownership.
use super::lane_matching::{LaneCompatibility, LaneEndpointIndex, maximum_lane_matching};
use super::{BYTE_DOMAIN, BYTE_DOMAIN_MASK_BYTES};
use crate::global::const_dsl::{
    receive_lane_causality::owned_receive_lane_conflict_from, receive_pair_is_causal,
};
use crate::{
    g::SourceActorImage,
    global::const_dsl::{
        EffList, ScopeKind, parallel_arm_ranges_from_enter, receive_lane_conflict,
        route_arm_ranges_from_first_enter, validate_receive_lane_causality,
    },
};

const fn inspect<const E: usize>(
    events: &EffList<E>,
    actors: Option<&SourceActorImage<E>>,
    resume: Option<(usize, Option<u16>)>,
    #[cfg(hibana_profile)] counters: &mut crate::g::profiling::Counters,
) -> Result<(), Option<(usize, usize, Option<u16>)>> {
    #[cfg(hibana_profile)]
    if resume.is_some() && actors.is_some() {
        counters.resumed_inspections += 1;
    } else {
        counters.full_inspections += 1;
    }
    match actors {
        Some(image) => match owned_receive_lane_conflict_from(events, image, resume) {
            Some(pair) => Err(Some(pair)),
            None => Ok(()),
        },
        None => {
            if validate_receive_lane_causality(events) {
                Ok(())
            } else {
                Err(receive_lane_conflict(events))
            }
        }
    }
}

const fn align_range<const E: usize>(
    events: &mut EffList<E>,
    actors: Option<&SourceActorImage<E>>,
    arm: (usize, usize),
    span: u16,
    colors: u16,
    #[cfg(hibana_profile)] counters: &mut crate::g::profiling::Counters,
) -> bool {
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
    let markers = events.scope_markers();
    let mut marker_index = 0;
    while marker_index < markers.len() {
        let marker = markers.at(marker_index);
        if marker.event.is_primary_enter()
            && matches!(marker.scope_id.kind(), Some(ScopeKind::Parallel))
        {
            let Some((start, split, _, end)) =
                parallel_arm_ranges_from_enter(markers, marker_index)
            else {
                crate::invariant()
            };
            let sibling = if start <= arm.0 && arm.1 <= split {
                Some((split, end))
            } else if split <= arm.0 && arm.1 <= end {
                Some((start, split))
            } else {
                None
            };
            if let Some((start, end)) = sibling {
                let siblings = LaneEndpointIndex::<BYTE_DOMAIN_MASK_BYTES>::from_range(
                    events, actors, start, end,
                );
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
        }
        marker_index += 1;
    }
    // An injective map preserves constraints inside the range. Check only
    // edges still possible after parallel ownership; clearing an edge is final.
    let mut inside = arm.0;
    while inside < arm.1 {
        let a = events.atom_at(inside);
        if a.from != a.to {
            let mut outside = 0;
            while outside < events.len() {
                if outside < arm.0 || arm.1 <= outside {
                    let b = events.atom_at(outside);
                    if b.from != b.to
                        && a.to == b.to
                        && a.from != b.from
                        && edges[a.lane as usize * row_bytes + b.lane as usize / 8]
                            & (1 << (b.lane % 8))
                            != 0
                        && (!receive_pair_is_causal(events, actors, inside, outside)
                            || !receive_pair_is_causal(events, actors, outside, inside))
                    {
                        edges[a.lane as usize * row_bytes + b.lane as usize / 8] &=
                            !(1 << (b.lane % 8));
                    }
                }
                outside += 1;
            }
        }
        inside += 1;
    }
    let matching = maximum_lane_matching::<0>(
        LaneCompatibility::Constraints {
            rows: edges.split_at(span as usize * row_bytes).0,
            row_bytes,
        },
        colors,
        span,
    );
    let mut changed = false;
    lane = 0;
    while lane < span {
        if members.contains_lane(lane as u8) {
            let Some(color) = matching.left_for_right(lane) else {
                return false;
            };
            changed |= color != lane;
        }
        lane += 1;
    }
    if !changed {
        return false;
    }
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
    let mut checked = inspect(
        events,
        actors,
        None,
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
            Err(Some(pair)) => pair,
            Err(None) => return false,
        };
        let mut changed = false;
        let mut palette = 0;
        while palette != 2 && !changed {
            let colors = if palette == 0 {
                span
            } else {
                BYTE_DOMAIN as u16
            };
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
                                actors,
                                (start, end),
                                span,
                                colors,
                                #[cfg(hibana_profile)]
                                counters,
                            );
                        }
                    }
                }
            }
            if !changed {
                // Refine the conflicting occurrence when no whole choice arm
                // can move. The same constraints apply inside and outside routes.
                changed = align_range(
                    events,
                    actors,
                    (second, second + 1),
                    span,
                    colors,
                    #[cfg(hibana_profile)]
                    counters,
                );
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
        // the failed obligation, then validate the final coloring in full.
        checked = inspect(
            events,
            actors,
            Some((first, scope)),
            #[cfg(hibana_profile)]
            counters,
        );
        if checked.is_ok() {
            // The logical-role checker above already scanned the whole source.
            if actors.is_none() {
                return true;
            }
            return inspect(
                events,
                actors,
                None,
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
