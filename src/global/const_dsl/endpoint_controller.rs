use super::{
    EffList, eff,
    scope_ranges::{
        parallel_arm_ranges_from_enter, parallel_enter_at, route_arm_ranges_from_first_enter,
        route_enter_at,
    },
};

#[cfg(kani)]
mod kani;

pub(crate) const fn first_visible_controller_mask(
    eff_list: &EffList,
    start: usize,
    end: usize,
) -> u16 {
    let markers = eff_list.scope_markers();
    let mut idx = start;
    while idx < end && idx < eff_list.len() {
        if let Some(route_enter) = route_enter_at(markers, idx, end, 0) {
            let (_, arm0_start, arm0_end, _, arm1_start, arm1_end) =
                route_arm_ranges_from_first_enter(markers, route_enter);
            return first_visible_controller_mask(eff_list, arm0_start, arm0_end)
                | first_visible_controller_mask(eff_list, arm1_start, arm1_end);
        }
        if let Some(par_enter) = parallel_enter_at(markers, idx, end, 0) {
            let Some((arm0_start, arm0_end, arm1_start, arm1_end)) =
                parallel_arm_ranges_from_enter(markers, par_enter)
            else {
                return 0;
            };
            return first_visible_controller_mask(eff_list, arm0_start, arm0_end)
                | first_visible_controller_mask(eff_list, arm1_start, arm1_end);
        }

        let node = eff_list.node_at(idx);
        if matches!(node.kind, eff::EffKind::Atom) {
            let atom = node.atom_data();
            if atom.from >= crate::g::ROLE_DOMAIN_SIZE {
                return 0;
            }
            return 1u16 << atom.from;
        }
        idx += 1;
    }
    0
}

#[inline(always)]
pub(crate) const fn unique_controller_role(mask: u16) -> Option<u8> {
    if mask == 0 || (mask & (mask - 1)) != 0 {
        return None;
    }
    let mut role = 0u8;
    while role < u16::BITS as u8 {
        if (mask & (1u16 << role)) != 0 {
            return Some(role);
        }
        role += 1;
    }
    None
}
