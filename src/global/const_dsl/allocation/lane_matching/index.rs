use super::super::BYTE_DOMAIN;
use crate::global::const_dsl::EffList;

#[derive(Clone, Copy)]
pub(super) struct EndpointSet<const ROLE_BYTES: usize>([u8; ROLE_BYTES]);

impl<const ROLE_BYTES: usize> EndpointSet<ROLE_BYTES> {
    const EMPTY: Self = Self([0; ROLE_BYTES]);

    #[inline(always)]
    const fn insert(&mut self, role: u8) {
        let byte = (role >> 3) as usize;
        if byte >= ROLE_BYTES {
            panic!("endpoint role exceeds represented domain");
        }
        self.0[byte] |= 1u8 << (role & 7);
    }

    #[cfg(kani)]
    #[inline(always)]
    const fn contains(self, role: u8) -> bool {
        let byte = (role >> 3) as usize;
        byte < ROLE_BYTES && (self.0[byte] & (1u8 << (role & 7))) != 0
    }

    #[cfg(kani)]
    const fn from_low_role_mask(mask: u8) -> Self {
        if ROLE_BYTES == 0 {
            panic!("endpoint role domain must not be empty");
        }
        let mut bytes = [0; ROLE_BYTES];
        bytes[0] = mask;
        Self(bytes)
    }

    #[cfg(kani)]
    const fn from_low_role_mask_u16(mask: u16) -> Self {
        if ROLE_BYTES < 2 {
            panic!("three-class matching requires two role-mask bytes");
        }
        let mut bytes = [0; ROLE_BYTES];
        bytes[0] = mask as u8;
        bytes[1] = (mask >> 8) as u8;
        Self(bytes)
    }

    #[inline(always)]
    pub(super) const fn is_disjoint(self, other: Self) -> bool {
        let mut byte = 0;
        while byte < ROLE_BYTES {
            if self.0[byte] & other.0[byte] != 0 {
                return false;
            }
            byte += 1;
        }
        true
    }
}

/// Lane lookup and endpoint-role sets are bounded by the complete wire lane
/// domain. Source event count does not allocate matching state.
pub(in super::super) struct LaneEndpointIndex<const ROLE_BYTES: usize> {
    slot_by_lane: [u16; BYTE_DOMAIN],
    endpoints: [EndpointSet<ROLE_BYTES>; BYTE_DOMAIN],
    len: u16,
}

impl<const ROLE_BYTES: usize> LaneEndpointIndex<ROLE_BYTES> {
    pub(in super::super) const fn from_range<const E: usize>(
        eff_list: &EffList<E>,
        start: usize,
        end: usize,
    ) -> Self {
        if E > u16::MAX as usize || start > end || end > eff_list.len() {
            panic!("parallel lane index range exceeds compact event domain");
        }

        let mut index = Self {
            slot_by_lane: [u16::MAX; BYTE_DOMAIN],
            endpoints: [EndpointSet::EMPTY; BYTE_DOMAIN],
            len: 0,
        };
        let mut idx = start;
        while idx < end {
            let atom = eff_list.atom_at(idx);
            let lane = atom.lane as usize;
            let slot = if index.slot_by_lane[lane] == u16::MAX {
                if index.len as usize >= BYTE_DOMAIN {
                    panic!("parallel lane index exceeds wire domain");
                }
                let slot = index.len;
                index.len += 1;
                index.slot_by_lane[lane] = slot;
                slot
            } else {
                index.slot_by_lane[lane]
            };
            index.endpoints[slot as usize].insert(atom.from);
            index.endpoints[slot as usize].insert(atom.to);
            idx += 1;
        }
        index
    }

    pub(super) const fn endpoint_set(&self, lane: u8) -> EndpointSet<ROLE_BYTES> {
        let slot = self.slot_by_lane[lane as usize];
        if slot == u16::MAX {
            EndpointSet::EMPTY
        } else {
            self.endpoints[slot as usize]
        }
    }

    pub(super) const fn contains_lane(&self, lane: u8) -> bool {
        self.slot_by_lane[lane as usize] != u16::MAX
    }

    #[cfg(kani)]
    pub(in super::super) const fn contains_role(&self, lane: u8, role: u8) -> bool {
        self.endpoint_set(lane).contains(role)
    }

    #[cfg(kani)]
    pub(in super::super) const fn lane_is_disjoint_from(
        &self,
        lane: u8,
        other: &Self,
        other_lane: u8,
    ) -> bool {
        self.endpoint_set(lane)
            .is_disjoint(other.endpoint_set(other_lane))
    }
}

#[cfg(kani)]
impl<const ROLE_BYTES: usize> LaneEndpointIndex<ROLE_BYTES> {
    pub(in super::super) const fn from_two_role_masks(first: u8, second: u8) -> Self {
        let mut slot_by_lane = [u16::MAX; BYTE_DOMAIN];
        slot_by_lane[0] = 0;
        slot_by_lane[1] = 1;
        Self {
            slot_by_lane,
            endpoints: {
                let mut endpoints = [EndpointSet::EMPTY; BYTE_DOMAIN];
                endpoints[0] = EndpointSet::from_low_role_mask(first);
                endpoints[1] = EndpointSet::from_low_role_mask(second);
                endpoints
            },
            len: 2,
        }
    }

    pub(in super::super) const fn from_three_role_masks(masks: [u16; 3]) -> Self {
        let mut slot_by_lane = [u16::MAX; BYTE_DOMAIN];
        slot_by_lane[0] = 0;
        slot_by_lane[1] = 1;
        slot_by_lane[2] = 2;
        Self {
            slot_by_lane,
            endpoints: {
                let mut endpoints = [EndpointSet::EMPTY; BYTE_DOMAIN];
                endpoints[0] = EndpointSet::from_low_role_mask_u16(masks[0]);
                endpoints[1] = EndpointSet::from_low_role_mask_u16(masks[1]);
                endpoints[2] = EndpointSet::from_low_role_mask_u16(masks[2]);
                endpoints
            },
            len: 3,
        }
    }
}
