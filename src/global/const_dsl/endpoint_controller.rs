use super::{
    EffList, eff,
    scope_ranges::{
        parallel_arm_ranges_from_enter, parallel_enter_at, route_arm_ranges_from_first_enter,
        route_enter_at,
    },
};

#[cfg(kani)]
mod kani;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum FirstVisibleController {
    Absent,
    Unique(u8),
    Ambiguous,
}

impl FirstVisibleController {
    pub(crate) const fn merge(self, other: Self) -> Self {
        match (self, other) {
            (Self::Absent, candidate) | (candidate, Self::Absent) => candidate,
            (Self::Unique(left), Self::Unique(right)) if left == right => Self::Unique(left),
            (Self::Unique(_), Self::Unique(_)) | (Self::Ambiguous, _) | (_, Self::Ambiguous) => {
                Self::Ambiguous
            }
        }
    }

    pub(crate) const fn unique(self) -> Option<u8> {
        match self {
            Self::Unique(role) => Some(role),
            Self::Absent | Self::Ambiguous => None,
        }
    }
}

pub(crate) const fn first_visible_controller(
    eff_list: &EffList,
    start: usize,
    end: usize,
) -> FirstVisibleController {
    let markers = eff_list.scope_markers();
    let mut idx = start;
    while idx < end && idx < eff_list.len() {
        if let Some(route_enter) = route_enter_at(markers, idx, end, 0) {
            let (_, arm0_start, arm0_end, _, arm1_start, arm1_end) =
                route_arm_ranges_from_first_enter(markers, route_enter);
            return first_visible_controller(eff_list, arm0_start, arm0_end)
                .merge(first_visible_controller(eff_list, arm1_start, arm1_end));
        }
        if let Some(par_enter) = parallel_enter_at(markers, idx, end, 0) {
            let Some((arm0_start, arm0_end, arm1_start, arm1_end)) =
                parallel_arm_ranges_from_enter(markers, par_enter)
            else {
                return FirstVisibleController::Absent;
            };
            return first_visible_controller(eff_list, arm0_start, arm0_end)
                .merge(first_visible_controller(eff_list, arm1_start, arm1_end));
        }

        let node = eff_list.node_at(idx);
        if matches!(node.kind, eff::EffKind::Atom) {
            return FirstVisibleController::Unique(node.atom_data().from);
        }
        idx += 1;
    }
    FirstVisibleController::Absent
}
