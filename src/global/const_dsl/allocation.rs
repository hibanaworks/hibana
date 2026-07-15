mod frame_labels;
#[cfg(kani)]
mod kani;
mod lane_matching;
#[cfg(all(test, hibana_repo_tests))]
mod tests;

pub(crate) use frame_labels::{color_roll_frame_labels, merge_route_frame_labels};
pub(crate) use lane_matching::merge_parallel_lanes;

const BYTE_DOMAIN: usize = u8::MAX as usize + 1;
const BYTE_DOMAIN_MASK_BYTES: usize = BYTE_DOMAIN.div_ceil(u8::BITS as usize);
