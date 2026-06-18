use crate::{eff, g::ProgramSourceError, global::const_dsl::EffList};

#[derive(Clone, Copy)]
struct BranchSig {
    from: u8,
    to: u8,
    label: u8,
    lane: u8,
}

impl BranchSig {
    const EMPTY: Self = Self {
        from: 0,
        to: 0,
        label: 0,
        lane: 0,
    };

    #[inline(always)]
    const fn same(self, other: Self) -> bool {
        self.from == other.from
            && self.to == other.to
            && self.label == other.label
            && self.lane == other.lane
    }
}

#[derive(Clone, Copy)]
struct FirstVisibleFrontier {
    sigs: [BranchSig; eff::meta::MAX_EFF_NODES],
    len: usize,
    controller_mask: u16,
    invalid_controller: bool,
}

impl FirstVisibleFrontier {
    const EMPTY: Self = Self {
        sigs: [BranchSig::EMPTY; eff::meta::MAX_EFF_NODES],
        len: 0,
        controller_mask: 0,
        invalid_controller: false,
    };
}

pub(super) const fn validate_intrinsic_first_visible_frontier<const ROLE: u8>(
    eff_list: &EffList,
    arm0_start: usize,
    arm0_end: usize,
    arm1_start: usize,
    arm1_end: usize,
) -> Option<ProgramSourceError> {
    let left = collect_first_visible_frontier(eff_list, arm0_start, arm0_end);
    let right = collect_first_visible_frontier(eff_list, arm1_start, arm1_end);
    if left.len == 0 || right.len == 0 || left.invalid_controller || right.invalid_controller {
        return Some(ProgramSourceError::ProjectionRouteUnprojectable);
    }
    let controller_mask = left.controller_mask | right.controller_mask;
    if !has_exactly_one_bit(controller_mask) {
        return Some(ProgramSourceError::RouteControllerMismatch);
    }
    if branch_signatures_overlap(&left, &right) {
        return Some(ProgramSourceError::ProjectionRouteUnprojectable);
    }
    if passive_first_visible_ambiguous::<ROLE>(&left, &right) {
        return Some(ProgramSourceError::ProjectionRouteUnprojectable);
    }
    None
}

const fn collect_first_visible_frontier(
    eff_list: &EffList,
    start: usize,
    end: usize,
) -> FirstVisibleFrontier {
    let mut frontier = FirstVisibleFrontier::EMPTY;
    let mut seen_lane_words = [0u64; 4];
    let mut idx = start;
    while idx < end && idx < eff_list.len() {
        let node = eff_list.node_at(idx);
        if matches!(node.kind, eff::EffKind::Atom) {
            let atom = node.atom_data();
            let lane = atom.lane as usize;
            let word = lane / 64;
            let bit = lane % 64;
            if word >= seen_lane_words.len() {
                frontier.invalid_controller = true;
                return frontier;
            }
            let mask = 1u64 << bit;
            if (seen_lane_words[word] & mask) != 0 {
                return frontier;
            }
            seen_lane_words[word] |= mask;
            if frontier.len >= eff::meta::MAX_EFF_NODES {
                panic!("first-visible frontier capacity exceeded");
            }
            frontier.sigs[frontier.len] = BranchSig {
                from: atom.from,
                to: atom.to,
                label: atom.label,
                lane: atom.lane,
            };
            frontier.len += 1;
            if atom.from >= u16::BITS as u8 {
                frontier.invalid_controller = true;
            } else {
                frontier.controller_mask |= 1u16 << atom.from;
            }
        }
        idx += 1;
    }
    frontier
}

#[inline(always)]
pub(super) const fn has_exactly_one_bit(mask: u16) -> bool {
    mask != 0 && (mask & (mask - 1)) == 0
}

const fn branch_signatures_overlap(
    left: &FirstVisibleFrontier,
    right: &FirstVisibleFrontier,
) -> bool {
    let mut left_idx = 0usize;
    while left_idx < left.len {
        let mut right_idx = 0usize;
        while right_idx < right.len {
            if left.sigs[left_idx].same(right.sigs[right_idx]) {
                return true;
            }
            right_idx += 1;
        }
        left_idx += 1;
    }
    false
}

const fn passive_first_visible_ambiguous<const ROLE: u8>(
    left: &FirstVisibleFrontier,
    right: &FirstVisibleFrontier,
) -> bool {
    let mut left_idx = 0usize;
    while left_idx < left.len {
        let lhs = left.sigs[left_idx];
        if lhs.to == ROLE && lhs.from != ROLE {
            let mut right_idx = 0usize;
            while right_idx < right.len {
                let rhs = right.sigs[right_idx];
                if rhs.to == ROLE
                    && rhs.from == lhs.from
                    && rhs.label == lhs.label
                    && rhs.lane == lhs.lane
                {
                    return true;
                }
                right_idx += 1;
            }
        }
        left_idx += 1;
    }
    false
}
