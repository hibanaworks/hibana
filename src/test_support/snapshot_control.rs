use crate::control::cap::mint::{CAP_HANDLE_LEN, CapShot, ControlOp, LocalControlKind};
use crate::control::types::{Lane, SessionId};
use crate::global::const_dsl::{ControlScopeKind, ScopeId};

pub(crate) const SNAPSHOT_CONTROL_LOGICAL: u8 = 125;
const TAG_STATE_SNAPSHOT_CONTROL: u8 = 0x42;

fn encode_session_lane_handle(sid: SessionId, lane: Lane) -> [u8; CAP_HANDLE_LEN] {
    let mut buf = [0u8; CAP_HANDLE_LEN];
    buf[0..4].copy_from_slice(&sid.raw().to_le_bytes());
    buf[4..6].copy_from_slice(&(lane.raw() as u16).to_le_bytes());
    buf
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SnapshotControl;

impl LocalControlKind for SnapshotControl {
    const TAG: u8 = TAG_STATE_SNAPSHOT_CONTROL;
    const SCOPE: ControlScopeKind = ControlScopeKind::State;
    const TAP_ID: u16 = crate::observe::ids::STATE_SNAPSHOT_REQ;
    const SHOT: CapShot = CapShot::One;
    const OP: ControlOp = ControlOp::StateSnapshot;

    fn encode_local_handle(sid: SessionId, lane: Lane, _scope: ScopeId) -> [u8; CAP_HANDLE_LEN] {
        encode_session_lane_handle(sid, lane)
    }
}
