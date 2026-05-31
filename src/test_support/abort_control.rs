use crate::control::cap::atomic_codecs::{
    TAG_ABORT_BEGIN_CONTROL, encode_session_lane_handle, mint_session_lane_handle,
};
use crate::control::cap::mint::{CAP_HANDLE_LEN, CapShot, ControlOp, LocalControlKind};
use crate::control::types::{Lane, SessionId};
use crate::global::const_dsl::{ControlScopeKind, ScopeId};

pub(crate) const ABORT_CONTROL_LOGICAL: u8 = 124;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AbortControl;

impl LocalControlKind for AbortControl {
    const TAG: u8 = TAG_ABORT_BEGIN_CONTROL;
    const SCOPE: ControlScopeKind = ControlScopeKind::Abort;
    const TAP_ID: u16 = crate::observe::ids::ABORT_BEGIN;
    const SHOT: CapShot = CapShot::One;
    const OP: ControlOp = ControlOp::AbortBegin;

    fn encode_local_handle(sid: SessionId, lane: Lane, _scope: ScopeId) -> [u8; CAP_HANDLE_LEN] {
        encode_session_lane_handle(mint_session_lane_handle(sid, lane))
    }
}
