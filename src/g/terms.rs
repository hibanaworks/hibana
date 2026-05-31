use super::{
    LoopControlMeaning, Par, Policy, ProgramSourceData, ProgramSourceError, ProgramTerm, Route,
    Send, Seq,
};
use crate::global::steps::{PolicyEligible, RoleLaneMask};

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

impl<From, To, M, const LANE: u8> ProgramTerm for Send<From, To, M, LANE>
where
    From: crate::global::KnownRole + crate::global::RoleMarker,
    To: crate::global::KnownRole + crate::global::RoleMarker,
    M: crate::global::MessageSpec,
{
    type Source = ProgramSourceData;
    const PROGRAM_SOURCE: Self::Source = {
        let control = <M as crate::global::MessageRuntime>::CONTROL;
        ProgramSourceData::from_parts(
            crate::global::const_dsl::const_send_typed::<From, To, M, LANE>(),
            RoleLaneMask::empty()
                .with_role(<From as crate::global::KnownRole>::INDEX, LANE)
                .with_role(<To as crate::global::KnownRole>::INDEX, LANE),
            false,
            LoopControlMeaning::from_control_spec(control).is_some(),
        )
    };
}

impl<Left, Right> ProgramTerm for Seq<Left, Right>
where
    Left: ProgramTerm<Source = ProgramSourceData>,
    Right: ProgramTerm<Source = ProgramSourceData>,
{
    type Source = ProgramSourceData;
    const PROGRAM_SOURCE: Self::Source =
        <Left as ProgramTerm>::PROGRAM_SOURCE.seq(<Right as ProgramTerm>::PROGRAM_SOURCE);
}

impl<Left, Right> ProgramTerm for Route<Left, Right>
where
    Left: ProgramTerm<Source = ProgramSourceData>,
    Right: ProgramTerm<Source = ProgramSourceData>,
{
    type Source = ProgramSourceData;
    const PROGRAM_SOURCE: Self::Source = {
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
    Left: ProgramTerm<Source = ProgramSourceData>,
    Right: ProgramTerm<Source = ProgramSourceData>,
{
    type Source = ProgramSourceData;
    const PROGRAM_SOURCE: Self::Source =
        { <Left as ProgramTerm>::PROGRAM_SOURCE.par(<Right as ProgramTerm>::PROGRAM_SOURCE) };
}

impl<Steps, const POLICY_ID: u16> ProgramTerm for Policy<Steps, POLICY_ID>
where
    Steps: ProgramTerm<Source = ProgramSourceData> + PolicyEligible,
{
    type Source = ProgramSourceData;
    const PROGRAM_SOURCE: Self::Source =
        <Steps as ProgramTerm>::PROGRAM_SOURCE.with_policy(POLICY_ID);
}
