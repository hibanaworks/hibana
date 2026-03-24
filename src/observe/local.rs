//! Local action failure normalisation helpers.
use crate::eff::EffIndex;
use crate::endpoint::LocalFailureReason;
use crate::observe::core::TapEvent;
use crate::observe::ids;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct LocalActionFailure {
    sid: u32,
    eff_index: EffIndex,
    pub(super) reason: LocalFailureReason,
}

impl LocalActionFailure {
    #[inline]
    pub(super) fn from_tap(event: TapEvent) -> Option<Self> {
        if event.id != ids::LOCAL_ACTION_FAIL {
            return None;
        }
        let sid = event.arg0;
        let eff_index = EffIndex::new((event.arg1 >> 16) as u16);
        let reason = LocalFailureReason::from_raw((event.arg1 & 0xFFFF) as u16);
        Some(Self {
            sid,
            eff_index,
            reason,
        })
    }
}
