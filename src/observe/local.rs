//! Local action failure normalisation helpers.
use crate::eff::EffIndex;
use crate::endpoint::LocalFailureReason;
use crate::observe::{TapEvent, ids};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalActionFailure {
    pub sid: u32,
    pub eff_index: EffIndex,
    pub reason: LocalFailureReason,
}

impl LocalActionFailure {
    #[inline]
    pub fn from_tap(event: TapEvent) -> Option<Self> {
        if event.id != ids::LOCAL_ACTION_FAIL {
            return None;
        }
        let sid = event.arg0;
        let eff_index = (event.arg1 >> 16) as EffIndex;
        let reason = LocalFailureReason::from_raw((event.arg1 & 0xFFFF) as u16);
        Some(Self {
            sid,
            eff_index,
            reason,
        })
    }
}
