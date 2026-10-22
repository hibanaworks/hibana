#[cfg(kani)]
mod kani;
mod lane_matching;
mod receive_lanes;
#[cfg(all(test, hibana_repo_tests))]
mod tests;

pub(crate) use lane_matching::merge_parallel_lanes;
pub(crate) use receive_lanes::assign_receive_lanes;

const BYTE_DOMAIN: usize = u8::MAX as usize + 1;
const BYTE_DOMAIN_MASK_BYTES: usize = BYTE_DOMAIN.div_ceil(u8::BITS as usize);
