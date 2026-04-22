use crate::control::cap::mint::{
    CAP_HANDLE_LEN, CapError, CapShot, ControlOp, ControlPath, ControlResourceKind, ResourceKind,
};
use crate::control::cap::atomic_codecs::{
    SessionLaneHandle, TAG_ABORT_BEGIN_CONTROL, decode_session_lane_handle,
    encode_session_lane_handle, mint_session_lane_handle,
};
use crate::control::types::{Lane, SessionId};
use crate::global::const_dsl::{ControlScopeKind, ScopeId};

pub(crate) const LABEL_ABORT_CONTROL: u8 = 124;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AbortControl;

impl ResourceKind for AbortControl {
    type Handle = SessionLaneHandle;
    const TAG: u8 = TAG_ABORT_BEGIN_CONTROL;
    const NAME: &'static str = "AbortControl";

    fn encode_handle(handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN] {
        encode_session_lane_handle(*handle)
    }

    fn decode_handle(data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError> {
        decode_session_lane_handle(data)
    }

    fn zeroize(_handle: &mut Self::Handle) {}
}

impl ControlResourceKind for AbortControl {
    const LABEL: u8 = LABEL_ABORT_CONTROL;
    const SCOPE: ControlScopeKind = ControlScopeKind::Abort;
    const TAP_ID: u16 = crate::observe::ids::CANCEL_BEGIN;
    const SHOT: CapShot = CapShot::One;
    const PATH: ControlPath = ControlPath::Local;
    const OP: ControlOp = ControlOp::AbortBegin;
    const AUTO_MINT_WIRE: bool = false;

    fn mint_handle(sid: SessionId, lane: Lane, _scope: ScopeId) -> Self::Handle {
        mint_session_lane_handle(sid, lane)
    }
}
