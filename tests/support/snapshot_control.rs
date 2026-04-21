use crate::control::cap::mint::{
    CAP_HANDLE_LEN, CapError, CapShot, ControlOp, ControlPath, ControlResourceKind, ResourceKind,
};
use crate::control::cap::resource_kinds::{
    SessionLaneHandle, TAG_STATE_SNAPSHOT_CONTROL, decode_session_lane_handle,
    encode_session_lane_handle, mint_session_lane_handle,
};
use crate::control::types::{Lane, SessionId};
use crate::global::const_dsl::{ControlScopeKind, ScopeId};

pub(crate) const LABEL_SNAPSHOT_CONTROL: u8 = 125;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SnapshotControl;

impl ResourceKind for SnapshotControl {
    type Handle = SessionLaneHandle;
    const TAG: u8 = TAG_STATE_SNAPSHOT_CONTROL;
    const NAME: &'static str = "SnapshotControl";

    fn encode_handle(handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN] {
        encode_session_lane_handle(*handle)
    }

    fn decode_handle(data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError> {
        decode_session_lane_handle(data)
    }

    fn zeroize(_handle: &mut Self::Handle) {}
}

impl ControlResourceKind for SnapshotControl {
    const LABEL: u8 = LABEL_SNAPSHOT_CONTROL;
    const SCOPE: ControlScopeKind = ControlScopeKind::Checkpoint;
    const TAP_ID: u16 = crate::observe::ids::CHECKPOINT_REQ;
    const SHOT: CapShot = CapShot::One;
    const PATH: ControlPath = ControlPath::Local;
    const OP: ControlOp = ControlOp::StateSnapshot;
    const AUTO_MINT_WIRE: bool = false;

    fn mint_handle(sid: SessionId, lane: Lane, _scope: ScopeId) -> Self::Handle {
        mint_session_lane_handle(sid, lane)
    }
}
