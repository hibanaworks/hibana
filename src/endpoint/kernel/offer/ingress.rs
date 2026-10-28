//! Offer ingress collection and restore ownership.

use core::task::Poll;

use super::{OfferScopeProfile, OfferScopeSelection, OfferStagedIngress, OperationKernel};
use crate::{
    endpoint::{RecvError, RecvResult},
    global::role_program::{LANE_SET_VIEW_WORDS, LaneWord, lane_word_index},
    transport::Transport,
};

use crate::endpoint::kernel::lane_port;

#[cfg(kani)]
mod kani;
#[cfg(all(test, hibana_repo_tests))]
mod tests;

/// Ascending incoming directions from one read cut. These borrowed arrays
/// choose ports to poll; they never carry event admission across a callback.
fn incoming_offer_lanes<'a>(
    incoming: &'a [LaneWord; LANE_SET_VIEW_WORDS],
    deferred: &'a [LaneWord; LANE_SET_VIEW_WORDS],
    lane_limit: usize,
) -> impl Iterator<Item = usize> + 'a {
    incoming
        .iter()
        .zip(deferred)
        .enumerate()
        .flat_map(move |(word, (&incoming, &deferred))| {
            let mut remaining = incoming & !deferred;
            core::iter::from_fn(move || {
                if remaining == 0 {
                    return None;
                }
                let bit = remaining.trailing_zeros() as usize;
                remaining &= remaining - 1;
                let lane = word * LaneWord::BITS as usize + bit;
                (lane < lane_limit).then_some(lane)
            })
        })
}

#[derive(Clone, Copy)]
pub(super) struct OfferFrontierFacts {
    pub(super) selection: OfferScopeSelection,
    pub(super) profile: OfferScopeProfile,
    pub(super) ingress_mode: OfferIngressMode,
}

impl OfferFrontierFacts {
    #[inline]
    pub(super) const fn scope_id(self) -> crate::global::const_dsl::ScopeId {
        self.selection.scope_id
    }

    #[inline]
    pub(super) const fn offer_lane_idx(self) -> usize {
        self.selection.offer_lane as usize
    }
}

#[derive(Clone, Copy)]
pub(super) enum OfferIngressMode {
    ResolvedWithoutFrame,
    TransportFrame,
}

impl<'r, T> OperationKernel<'_, 'r, T>
where
    T: Transport + 'r,
{
    pub(super) fn await_transport_payload_for_offer_lane(
        &mut self,
        pending_recv: &mut lane_port::PendingRecv,
        offer_lane: u8,
        ingress: &mut OfferStagedIngress<'r>,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<()>> {
        if ingress.has_transport() {
            crate::invariant();
        }
        let frame = match self.poll_received_framed_transport_frame_for_lane(
            pending_recv,
            offer_lane as usize,
            offer_lane,
            cx,
        ) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Ok(frame)) => frame,
            Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
        };
        ingress.stage_transport(frame);
        Poll::Ready(Ok(()))
    }

    #[inline]
    pub(super) fn requeue_offer_transport_payload(
        &mut self,
        payload: lane_port::PreambleFrame<'r>,
    ) -> RecvResult<()> {
        let port = self.port_for_lane(payload.lane_idx());
        let requeue = payload.requeue_on(port);
        if let Some(kind) = self.session_fault() {
            return Err(RecvError::SessionFault(kind));
        }
        requeue.map_err(RecvError::Transport)
    }

    pub(super) fn collect_offer_ingress(
        &mut self,
        pending_recv: &mut lane_port::PendingRecv,
        facts: OfferFrontierFacts,
        cx: &mut core::task::Context<'_>,
        inspected_lanes: &mut [LaneWord; LANE_SET_VIEW_WORDS],
    ) -> Poll<RecvResult<Option<lane_port::PreambleFrame<'r>>>> {
        if matches!(facts.ingress_mode, OfferIngressMode::ResolvedWithoutFrame) {
            return Poll::Ready(Ok(None));
        }

        let frame = match self.poll_received_transport_frame_for_offer(
            pending_recv,
            facts,
            cx,
            inspected_lanes,
        ) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Ok(frame)) => frame,
            Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
        };

        Poll::Ready(Ok(Some(frame)))
    }

    pub(super) fn poll_any_active_offer_transport_frame(
        &mut self,
        pending_recv: &mut lane_port::PendingRecv,
        cx: &mut core::task::Context<'_>,
        inspected_lanes: &mut [LaneWord; LANE_SET_VIEW_WORDS],
        incoming: &[LaneWord; LANE_SET_VIEW_WORDS],
    ) -> Poll<RecvResult<Option<lane_port::PreambleFrame<'r>>>> {
        let lane_limit = self.progress.cursor.logical_lane_count();
        let previously_inspected = *inspected_lanes;
        for lane_idx in incoming_offer_lanes(incoming, &previously_inspected, lane_limit) {
            if !self.owns_lane(lane_idx) || self.io.ports.get(lane_idx).is_none_or(Option::is_none)
            {
                continue;
            }
            let (word, bit) = lane_word_index(lane_idx);
            inspected_lanes[word] |= bit;
            match self.poll_received_framed_transport_frame_for_lane(
                pending_recv,
                lane_idx,
                lane_idx as u8,
                cx,
            ) {
                Poll::Ready(Ok(frame)) => return Poll::Ready(Ok(Some(frame))),
                Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
                Poll::Pending => {}
            }
        }
        Poll::Ready(Ok(None))
    }

    fn poll_received_transport_frame_for_offer(
        &mut self,
        pending_recv: &mut lane_port::PendingRecv,
        facts: OfferFrontierFacts,
        cx: &mut core::task::Context<'_>,
        inspected_lanes: &mut [LaneWord; LANE_SET_VIEW_WORDS],
    ) -> Poll<RecvResult<lane_port::PreambleFrame<'r>>> {
        let lane_limit = self.progress.cursor.logical_lane_count();
        let lanes = self.transport_lane_set_for_offer(facts);
        let preferred_lane = self.preferred_transport_lane_for_offer(facts);
        let mut lanes = OfferLaneScan::new(preferred_lane, lanes, lane_limit);
        while let Some(lane_idx) = lanes.next() {
            let (word, bit) = lane_word_index(lane_idx);
            if !self.owns_lane(lane_idx)
                || inspected_lanes[word] & bit != 0
                || self.io.ports.get(lane_idx).is_none_or(Option::is_none)
            {
                continue;
            }
            inspected_lanes[word] |= bit;
            match self.poll_received_framed_transport_frame_for_lane(
                pending_recv,
                lane_idx,
                lane_idx as u8,
                cx,
            ) {
                Poll::Pending => continue,
                Poll::Ready(Ok(frame)) => return Poll::Ready(Ok(frame)),
                Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
            }
        }
        Poll::Pending
    }

    fn transport_lane_set_for_offer(
        &self,
        facts: OfferFrontierFacts,
    ) -> crate::global::role_program::LaneSetView<'static> {
        self.offer_lane_set_for_scope(facts.scope_id())
    }

    fn preferred_transport_lane_for_offer(&self, facts: OfferFrontierFacts) -> usize {
        facts.offer_lane_idx()
    }
}

#[derive(Clone, Copy)]
enum OfferLaneScanCursor {
    Preferred(usize),
    Remaining(usize),
    Exhausted,
}

struct OfferLaneScan<'a> {
    offer_lanes: crate::global::role_program::LaneSetView<'a>,
    lane_limit: usize,
    preferred_lane: Option<usize>,
    cursor: OfferLaneScanCursor,
}

impl<'a> OfferLaneScan<'a> {
    #[inline]
    fn new(
        preferred_lane: usize,
        offer_lanes: crate::global::role_program::LaneSetView<'a>,
        lane_limit: usize,
    ) -> Self {
        let preferred_lane = if preferred_lane < lane_limit && offer_lanes.contains(preferred_lane)
        {
            Some(preferred_lane)
        } else {
            None
        };
        let cursor = match preferred_lane {
            Some(lane) => OfferLaneScanCursor::Preferred(lane),
            None => OfferLaneScanCursor::Remaining(0),
        };
        Self {
            offer_lanes,
            lane_limit,
            preferred_lane,
            cursor,
        }
    }

    fn next(&mut self) -> Option<usize> {
        loop {
            match self.cursor {
                OfferLaneScanCursor::Preferred(lane) => {
                    self.cursor = OfferLaneScanCursor::Remaining(0);
                    return Some(lane);
                }
                OfferLaneScanCursor::Remaining(start) => {
                    let Some(lane) = self.offer_lanes.next_set_from(start, self.lane_limit) else {
                        self.cursor = OfferLaneScanCursor::Exhausted;
                        return None;
                    };
                    self.cursor = OfferLaneScanCursor::Remaining(lane + 1);
                    if self.preferred_lane != Some(lane) {
                        return Some(lane);
                    }
                }
                OfferLaneScanCursor::Exhausted => return None,
            }
        }
    }
}
