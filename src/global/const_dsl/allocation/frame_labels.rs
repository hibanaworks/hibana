mod roll;
mod route;

pub(crate) use roll::color_roll_frame_labels;
pub(crate) use route::merge_route_frame_labels;

use super::{BYTE_DOMAIN, BYTE_DOMAIN_MASK_BYTES};

#[inline(always)]
const fn insert(mask: &mut [u8; BYTE_DOMAIN_MASK_BYTES], value: u8) {
    mask[(value >> 3) as usize] |= 1u8 << (value & 7);
}

#[inline(always)]
const fn contains(mask: &[u8; BYTE_DOMAIN_MASK_BYTES], value: u8) -> bool {
    (mask[(value >> 3) as usize] & (1u8 << (value & 7))) != 0
}

const fn first_available(mask: &[u8; BYTE_DOMAIN_MASK_BYTES]) -> Option<u8> {
    let mut value = 0usize;
    while value < BYTE_DOMAIN {
        let encoded = value as u8;
        if !contains(mask, encoded) {
            return Some(encoded);
        }
        value += 1;
    }
    None
}
