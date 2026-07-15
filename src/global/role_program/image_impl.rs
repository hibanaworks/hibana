mod blob_image;
mod event_rows;
#[cfg(kani)]
mod kani;
mod lane_image;
mod plan;
mod projection;
mod ref_access;
#[cfg(all(test, hibana_repo_tests))]
mod tests;

#[inline(always)]
const fn decode_binary_route_arm_index(arm: u8) -> Option<usize> {
    match arm {
        0 => Some(0),
        1 => Some(1),
        2..=u8::MAX => None,
    }
}

#[inline(never)]
const fn binary_route_arm_index(arm: u8) -> usize {
    match decode_binary_route_arm_index(arm) {
        Some(index) => index,
        None => crate::invariant(),
    }
}

#[inline(never)]
const fn route_arm_row_index(slot: usize, arm: u8) -> usize {
    let arm = binary_route_arm_index(arm);
    let Some(base) = slot.checked_mul(2) else {
        crate::invariant();
    };
    let Some(row) = base.checked_add(arm) else {
        crate::invariant();
    };
    row
}
