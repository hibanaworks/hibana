use crate::eff::EffAtom;

const LANE_DOMAIN_SIZE: usize = u8::MAX as usize + 1;
const FRAME_LABEL_COUNTERS: usize = crate::g::ROLE_DOMAIN_SIZE as usize * LANE_DOMAIN_SIZE;

#[derive(Clone, Copy)]
pub(crate) struct FrameLabelKey {
    target: u8,
    lane: u8,
}

impl FrameLabelKey {
    #[inline(always)]
    pub(crate) const fn from_atom(atom: EffAtom) -> Self {
        Self {
            target: atom.to,
            lane: atom.lane,
        }
    }

    #[inline(always)]
    const fn counter_index(self) -> usize {
        let target = self.target as usize;
        if target >= crate::g::ROLE_DOMAIN_SIZE as usize {
            panic!("frame label target role out of domain");
        }
        target * LANE_DOMAIN_SIZE + self.lane as usize
    }
}

#[inline(always)]
pub(crate) const fn frame_label_from_prior_count(count: u16) -> u8 {
    if count > u8::MAX as u16 {
        panic!("frame label domain overflow");
    }
    count as u8
}

pub(crate) struct FrameLabelAssigner {
    counts: [u16; FRAME_LABEL_COUNTERS],
}

impl FrameLabelAssigner {
    pub(crate) const EMPTY: Self = Self {
        counts: [0; FRAME_LABEL_COUNTERS],
    };

    #[inline(always)]
    pub(crate) const fn assign(&mut self, atom: EffAtom) -> u8 {
        let key = FrameLabelKey::from_atom(atom);
        let idx = key.counter_index();
        let label = frame_label_from_prior_count(self.counts[idx]);
        self.counts[idx] += 1;
        label
    }
}
