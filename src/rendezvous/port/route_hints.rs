//! Lane-local route hint queue.

use crate::transport::FrameLabelMask;

#[derive(Clone, Copy)]
pub(super) struct RouteHintQueue {
    pub(super) present_mask: FrameLabelMask,
}

impl RouteHintQueue {
    #[inline]
    pub(super) const fn from_mask(present_mask: FrameLabelMask) -> Self {
        Self { present_mask }
    }

    #[inline]
    pub(super) fn clear(&mut self) {
        self.present_mask = FrameLabelMask::EMPTY;
    }

    #[inline]
    pub(super) fn take_from_frame_label_mask(
        &mut self,
        frame_label_mask: FrameLabelMask,
    ) -> Option<u8> {
        self.present_mask
            .take_matching(|frame_label| frame_label_mask.contains_frame_label(frame_label))
    }

    #[inline]
    pub(super) fn has_any_frame_label_in_mask(&self, frame_label_mask: FrameLabelMask) -> bool {
        self.present_mask.intersects(frame_label_mask)
    }
}
