use super::*;

/// Add source witnesses only after the unchanged acceptance gate has rejected.
pub(crate) const fn projection_diagnostic<const E: usize>(
    summary: &CompiledProgramImage,
    source: &crate::g::ProgramSourceData<E>,
    error: ProgramSourceError,
) -> crate::g::ProjectionDiagnostic {
    use crate::g::{DiagnosticEvent, DiagnosticRange, ProjectionDiagnostic, ProjectionProblem};
    let eff_list = source.eff_list();
    let actors = match source.actors() {
        Some(actors) => Some(actors.event_actors()),
        None => None,
    };
    let mut diagnostic = ProjectionDiagnostic::from_error(error);
    if matches!(error, ProgramSourceError::ReceiveLaneCausalityConflict) {
        let conflict = match crate::global::const_dsl::receive_lane_conflict(eff_list) {
            Some(pair) => Some(pair),
            None => crate::global::const_dsl::passive_choice_conflict::<E, 0>(eff_list, None, None),
        };
        if let Some((earlier, later, scope)) = conflict {
            diagnostic.role = Some(eff_list.atom_at(earlier).to);
            diagnostic.scope = scope;
            diagnostic.first = Some(DiagnosticEvent::from_atom(
                earlier,
                eff_list.atom_at(earlier),
            ));
            diagnostic.second = Some(DiagnosticEvent::from_atom(later, eff_list.atom_at(later)));
        }
        return diagnostic;
    }
    if !matches!(
        error,
        ProgramSourceError::RouteControllerMismatch
            | ProgramSourceError::ProjectionRouteUnprojectable
    ) {
        return diagnostic;
    }
    if !matches!(error, ProgramSourceError::RouteControllerMismatch)
        && passive_child::validate_passive_child_projection_guarantees(eff_list).is_some()
    {
        return diagnostic;
    }
    let participants = participating_roles(eff_list);
    let markers = eff_list.scope_markers();
    let mut marker_idx = 0;
    while marker_idx < markers.len() {
        let marker = markers.at(marker_idx);
        if matches!(marker.scope_id.kind(), Some(ScopeKind::Route))
            && marker.event.is_primary_enter()
            && if matches!(error, ProgramSourceError::RouteControllerMismatch) {
                matches!(
                    route_controller_and_selectors(eff_list, marker_idx, actors),
                    Err(ProgramSourceError::RouteControllerMismatch)
                )
            } else {
                validate_route_scope(
                    summary.compiled_program_role_count(),
                    &participants,
                    eff_list,
                    markers,
                    marker_idx,
                )
                .is_some()
            }
        {
            let [(a, b), (c, d)] = route_arm_ranges_from_first_enter(markers, marker_idx);
            diagnostic.scope = Some(marker.scope_id.local_ordinal());
            diagnostic.arms = Some([
                DiagnosticRange { start: a, end: b },
                DiagnosticRange { start: c, end: d },
            ]);
            let owners = if scope_has_dynamic_resolver(eff_list, marker.scope_id) {
                None
            } else {
                actors
            };
            let controller = match first_visible_controller(eff_list, (a, b), owners)
                .merge(first_visible_controller(eff_list, (c, d), owners))
                .unique()
            {
                Some(_) => Some(eff_list.atom_at(a).from),
                None => None,
            };
            diagnostic.controller = controller;
            if a < b {
                diagnostic.first = Some(DiagnosticEvent::from_atom(a, eff_list.atom_at(a)));
            }
            if c < d {
                diagnostic.second = Some(DiagnosticEvent::from_atom(c, eff_list.atom_at(c)));
            }

            if let Some(controller) = controller {
                let mut role = 0;
                while role < summary.compiled_program_role_count() {
                    if participants[role / 8] & (1u8 << (role % 8)) != 0
                        && role as u8 != controller
                        && !local_route_observer_paths_mergeable(eff_list, a, b, c, d, role as u8)
                    {
                        diagnostic.problem = ProjectionProblem::MissingBranchKnowledge;
                        diagnostic.role = Some(role as u8);
                        diagnostic.first = first_role_event(eff_list, a, b, role as u8);
                        diagnostic.second = first_role_event(eff_list, c, d, role as u8);
                        return diagnostic;
                    }
                    role += 1;
                }
            }
            return diagnostic;
        }
        marker_idx += 1;
    }
    diagnostic
}

const fn first_role_event<const E: usize>(
    eff_list: &EffList<E>,
    start: usize,
    end: usize,
    role: u8,
) -> Option<crate::g::DiagnosticEvent> {
    let mut index = start;
    while index < end {
        let atom = eff_list.atom_at(index);
        if atom.from == role || atom.to == role {
            return Some(crate::g::DiagnosticEvent::from_atom(index, atom));
        }
        index += 1;
    }
    None
}
