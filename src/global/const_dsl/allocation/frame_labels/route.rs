use super::{BYTE_DOMAIN, BYTE_DOMAIN_MASK_BYTES, first_available, insert};
use crate::{eff::EffKind, global::const_dsl::EffList};

/// Separate the frame-label classes of two route arms for each
/// source/receiver/lane key. Sequential composition does not call this function, so ordered
/// occurrences reuse labels regardless of total choreography length.
pub(crate) const fn merge_route_frame_labels<const E: usize>(
    eff_list: &mut EffList<E>,
    left_start: usize,
    right_start: usize,
    right_end: usize,
) {
    if left_start >= right_start || right_start >= right_end || right_end > eff_list.len() {
        panic!("route frame-label merge requires two non-empty arms");
    }

    let mut key_idx = right_start;
    while key_idx < right_end {
        let key_node = eff_list.node_at(key_idx);
        if matches!(key_node.kind, EffKind::Atom) {
            let key = key_node.atom_data();
            if key.from == key.to {
                key_idx += 1;
                continue;
            }
            let mut earlier_same_key = false;
            let mut prior_right = right_start;
            while prior_right < key_idx {
                let prior_node = eff_list.node_at(prior_right);
                if matches!(prior_node.kind, EffKind::Atom) {
                    let prior = prior_node.atom_data();
                    if prior.from == key.from && prior.to == key.to && prior.lane == key.lane {
                        earlier_same_key = true;
                        break;
                    }
                }
                prior_right += 1;
            }

            if !earlier_same_key {
                let mut used = [0u8; BYTE_DOMAIN_MASK_BYTES];
                let mut left_idx = left_start;
                while left_idx < right_start {
                    let left_node = eff_list.node_at(left_idx);
                    if matches!(left_node.kind, EffKind::Atom) {
                        let left = left_node.atom_data();
                        if left.from == key.from && left.to == key.to && left.lane == key.lane {
                            insert(&mut used, eff_list.frame_label_at(left_idx));
                        }
                    }
                    left_idx += 1;
                }

                let mut remap = [0u8; BYTE_DOMAIN];
                let mut mapped = [false; BYTE_DOMAIN];
                let mut right_idx = right_start;
                while right_idx < right_end {
                    let right_node = eff_list.node_at(right_idx);
                    if matches!(right_node.kind, EffKind::Atom) {
                        let right = right_node.atom_data();
                        if right.from == key.from && right.to == key.to && right.lane == key.lane {
                            let old = eff_list.frame_label_at(right_idx);
                            if !mapped[old as usize] {
                                let Some(new) = first_available(&used) else {
                                    panic!("route inbound occurrence coloring exceeds wire domain");
                                };
                                remap[old as usize] = new;
                                mapped[old as usize] = true;
                                insert(&mut used, new);
                            }
                        }
                    }
                    right_idx += 1;
                }

                right_idx = right_start;
                while right_idx < right_end {
                    let right_node = eff_list.node_at(right_idx);
                    if matches!(right_node.kind, EffKind::Atom) {
                        let right = right_node.atom_data();
                        if right.from == key.from && right.to == key.to && right.lane == key.lane {
                            let old = eff_list.frame_label_at(right_idx);
                            eff_list.set_frame_label(right_idx, remap[old as usize]);
                        }
                    }
                    right_idx += 1;
                }
            }
        }
        key_idx += 1;
    }
}
