use super::{Par, ProgramSourceData, ProgramSourceError, ProgramTerm, Resolve, Route, Send, Seq};
use crate::global::LoopControlMeaning;
use crate::global::steps::RoleLaneMask;

#[derive(Clone, Copy)]
struct CycleRouteValidation {
    is_cycle: bool,
    error: Option<ProgramSourceError>,
}

const fn is_binary_cycle_route(
    left: Option<LoopControlMeaning>,
    right: Option<LoopControlMeaning>,
) -> CycleRouteValidation {
    match (left, right) {
        (Some(LoopControlMeaning::Continue), Some(LoopControlMeaning::Break)) => {
            CycleRouteValidation {
                is_cycle: true,
                error: None,
            }
        }
        (Some(_), Some(_)) => CycleRouteValidation {
            is_cycle: false,
            error: Some(ProgramSourceError::LoopRouteArmOrder),
        },
        (Some(_), None) | (None, Some(_)) => CycleRouteValidation {
            is_cycle: false,
            error: Some(ProgramSourceError::LoopRouteArmPair),
        },
        _ => CycleRouteValidation {
            is_cycle: false,
            error: None,
        },
    }
}

impl<const FROM: u8, const TO: u8, M> ProgramTerm for Send<FROM, TO, M>
where
    M: crate::global::Message,
{
    const PROGRAM_SOURCE: ProgramSourceData = {
        let control = <M as crate::global::MessageRuntime>::CONTROL;
        ProgramSourceData::from_parts(
            crate::global::const_dsl::const_send_typed::<FROM, TO, M, 0>(),
            RoleLaneMask::empty().with_role(FROM, 0).with_role(TO, 0),
            1,
            false,
            LoopControlMeaning::from_control_spec(control).is_some(),
        )
    };
}

impl<Left, Right> ProgramTerm for Seq<Left, Right>
where
    Left: ProgramTerm,
    Right: ProgramTerm,
{
    const PROGRAM_SOURCE: ProgramSourceData =
        <Left as ProgramTerm>::PROGRAM_SOURCE.seq(<Right as ProgramTerm>::PROGRAM_SOURCE);
}

impl<Left, Right> ProgramTerm for Route<Left, Right>
where
    Left: ProgramTerm,
    Right: ProgramTerm,
{
    const PROGRAM_SOURCE: ProgramSourceData = {
        let left = <Left as ProgramTerm>::PROGRAM_SOURCE;
        let right = <Right as ProgramTerm>::PROGRAM_SOURCE;
        let left_head = left.route_head();
        let right_head = right.route_head();
        let mut route_error = ProgramSourceData::merge_error(left_head.error, right_head.error);
        if left_head.label == right_head.label {
            route_error = ProgramSourceData::merge_error(
                route_error,
                Some(ProgramSourceError::RouteDuplicateLabel),
            );
        }
        if left_head.controller != right_head.controller {
            route_error = ProgramSourceData::merge_error(
                route_error,
                Some(ProgramSourceError::RouteControllerMismatch),
            );
        }
        let cycle = is_binary_cycle_route(left_head.cycle_meaning, right_head.cycle_meaning);
        route_error = ProgramSourceData::merge_error(route_error, cycle.error);
        left.route_with_controller(right, left_head.controller, cycle.is_cycle, route_error)
    };
}

impl<Left, Right> ProgramTerm for Par<Left, Right>
where
    Left: ProgramTerm,
    Right: ProgramTerm,
{
    const PROGRAM_SOURCE: ProgramSourceData =
        { <Left as ProgramTerm>::PROGRAM_SOURCE.par(<Right as ProgramTerm>::PROGRAM_SOURCE) };
}

impl<Left, Right, const RESOLVER_ID: u16> ProgramTerm for Resolve<Route<Left, Right>, RESOLVER_ID>
where
    Left: ProgramTerm,
    Right: ProgramTerm,
{
    const PROGRAM_SOURCE: ProgramSourceData =
        <Route<Left, Right> as ProgramTerm>::PROGRAM_SOURCE.resolve_route(RESOLVER_ID);
}
