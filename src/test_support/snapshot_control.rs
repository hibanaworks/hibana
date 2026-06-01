use crate::control::cap::atomic_codecs::{
    TAG_STATE_SNAPSHOT_CONTROL, encode_session_lane_handle, mint_session_lane_handle,
};
use crate::control::cap::mint::{CAP_HANDLE_LEN, CapShot, ControlOp, LocalControlKind};
use crate::control::types::{Lane, SessionId};
use crate::global::const_dsl::{ControlScopeKind, ScopeId};

pub(crate) const SNAPSHOT_CONTROL_LOGICAL: u8 = 125;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SnapshotControl;

impl LocalControlKind for SnapshotControl {
    const TAG: u8 = TAG_STATE_SNAPSHOT_CONTROL;
    const SCOPE: ControlScopeKind = ControlScopeKind::State;
    const TAP_ID: u16 = crate::observe::ids::STATE_SNAPSHOT_REQ;
    const SHOT: CapShot = CapShot::One;
    const OP: ControlOp = ControlOp::StateSnapshot;

    fn encode_local_handle(sid: SessionId, lane: Lane, _scope: ScopeId) -> [u8; CAP_HANDLE_LEN] {
        encode_session_lane_handle(mint_session_lane_handle(sid, lane))
    }
}
