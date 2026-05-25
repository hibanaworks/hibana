//! Lane-local route hint queue.

use crate::transport::{FrameLabelMask, Transport};

pub(super) const ROUTE_HINT_SLOTS: usize = u8::MAX as usize + 1;

#[derive(Clone, Copy)]
pub(super) struct RouteHintQueue {
    pub(super) present_mask: FrameLabelMask,
}

impl RouteHintQueue {
    #[cfg(test)]
    #[inline]
    pub(super) const fn new() -> Self {
        Self {
            present_mask: FrameLabelMask::EMPTY,
        }
    }

    #[inline]
    pub(super) const fn from_mask(present_mask: FrameLabelMask) -> Self {
        Self { present_mask }
    }

    #[inline]
    pub(super) fn push(&mut self, frame_label: u8) {
        self.present_mask.insert_frame_label(frame_label);
    }

    #[inline]
    pub(super) fn clear(&mut self) {
        self.present_mask = FrameLabelMask::EMPTY;
    }

    #[inline]
    pub(super) fn drain_from_transport<'a, T: Transport>(
        &mut self,
        transport: &'a T,
        rx: &'a T::Rx<'a>,
    ) {
        let mut drained = 0usize;
        while drained < ROUTE_HINT_SLOTS {
            let Some(frame_label) = transport.recv_frame_hint(rx) else {
                return;
            };
            self.push(frame_label.raw());
            drained += 1;
        }
        assert!(
            transport.recv_frame_hint(rx).is_none(),
            "transport hint-drain must not yield more than the frame-label domain",
        );
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

    #[cfg(test)]
    #[inline]
    pub(super) fn take_matching<F>(&mut self, matches: F) -> Option<u8>
    where
        F: FnMut(u8) -> bool,
    {
        self.present_mask.take_matching(matches)
    }

    #[cfg(test)]
    #[inline]
    pub(super) fn has_matching<F>(&self, matches: F) -> bool
    where
        F: FnMut(u8) -> bool,
    {
        self.present_mask.has_matching(matches)
    }
}
