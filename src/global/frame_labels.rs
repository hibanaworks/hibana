use crate::{
    eff::{EffAtom, EffKind},
    global::const_dsl::EffList,
};

#[inline(always)]
pub(crate) const fn frame_label_from_prior_count(count: u16) -> u8 {
    if count > u8::MAX as u16 {
        panic!("frame label domain overflow");
    }
    count as u8
}

/// Derive a frame label from prior events with the same target and lane.
///
/// The event list is the sole authority. No role-domain-sized counter table is
/// retained in const evaluation or in the compiled image.
pub(crate) const fn frame_label_at(eff_list: &EffList, eff_idx: usize, atom: EffAtom) -> u8 {
    if eff_idx >= eff_list.len() {
        panic!("frame label event offset out of bounds");
    }
    let mut count = 0u16;
    let mut prior_idx = 0usize;
    while prior_idx < eff_idx {
        let prior = eff_list.node_at(prior_idx);
        if matches!(prior.kind, EffKind::Atom) {
            let prior = prior.atom_data();
            if prior.to == atom.to && prior.lane == atom.lane {
                if count == u16::MAX {
                    panic!("frame label count overflow");
                }
                count += 1;
            }
        }
        prior_idx += 1;
    }
    frame_label_from_prior_count(count)
}
