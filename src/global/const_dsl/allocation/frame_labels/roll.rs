use super::{BYTE_DOMAIN_MASK_BYTES, first_available, insert};
use crate::global::const_dsl::{EffList, event_relations::events_share_route_path};

/// Canonicalize one roll body by inbound operation key and exact route path.
/// Ordered occurrences on one path retain a shared color, while every path
/// that elastic reentry can expose concurrently receives a distinct color.
pub(crate) const fn color_roll_frame_labels<const E: usize>(
    eff_list: &mut EffList<E>,
    start: usize,
    end: usize,
) {
    if start >= end || end > eff_list.len() {
        panic!("roll frame-label coloring requires a non-empty body");
    }

    let mut class_idx = start;
    while class_idx < end {
        let class = eff_list.atom_at(class_idx);
        if class.from == class.to {
            class_idx += 1;
            continue;
        }

        let mut already_colored = false;
        let mut prior_idx = start;
        while prior_idx < class_idx {
            let prior = eff_list.atom_at(prior_idx);
            if prior.from == class.from
                && prior.to == class.to
                && prior.lane == class.lane
                && events_share_route_path(eff_list.scope_markers(), prior_idx, class_idx)
            {
                already_colored = true;
                break;
            }
            prior_idx += 1;
        }

        if !already_colored {
            let mut used = [0u8; BYTE_DOMAIN_MASK_BYTES];
            prior_idx = start;
            while prior_idx < class_idx {
                let prior = eff_list.atom_at(prior_idx);
                if prior.from == class.from && prior.to == class.to && prior.lane == class.lane {
                    insert(&mut used, eff_list.frame_label_at(prior_idx));
                }
                prior_idx += 1;
            }
            let Some(color) = first_available(&used) else {
                panic!("roll inbound occurrence coloring exceeds wire domain");
            };

            let mut member_idx = class_idx;
            while member_idx < end {
                let member = eff_list.atom_at(member_idx);
                if member.from == class.from
                    && member.to == class.to
                    && member.lane == class.lane
                    && events_share_route_path(eff_list.scope_markers(), class_idx, member_idx)
                {
                    eff_list.set_frame_label(member_idx, color);
                }
                member_idx += 1;
            }
        }
        class_idx += 1;
    }
}
