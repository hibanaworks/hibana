use crate::control::cap::mint::{
    CAP_HANDLE_LEN, CapError, CapShot, ControlOp, ControlPath, ControlResourceKind, ResourceKind,
};
use crate::control::cap::resource_kinds::{DelegationHandle, TAG_CAP_DELEGATE_CONTROL};
use crate::control::types::{Lane, SessionId};
use crate::global::const_dsl::{ControlScopeKind, ScopeId};

pub(crate) const LABEL_DELEGATE_CONTROL: u8 = 126;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DelegateControl;

impl ResourceKind for DelegateControl {
    type Handle = DelegationHandle;
    const TAG: u8 = TAG_CAP_DELEGATE_CONTROL;
    const NAME: &'static str = "DelegateControl";

    fn encode_handle(handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN] {
        handle.encode()
    }

    fn decode_handle(data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError> {
        DelegationHandle::decode(data)
    }

    fn zeroize(_handle: &mut Self::Handle) {}
}

impl ControlResourceKind for DelegateControl {
    const LABEL: u8 = LABEL_DELEGATE_CONTROL;
    const SCOPE: ControlScopeKind = ControlScopeKind::Reroute;
    const TAP_ID: u16 = crate::observe::ids::ROUTE_PICK;
    const SHOT: CapShot = CapShot::One;
    const PATH: ControlPath = ControlPath::Local;
    const OP: ControlOp = ControlOp::CapDelegate;
    const AUTO_MINT_WIRE: bool = false;

    fn mint_handle(_sid: SessionId, lane: Lane, _scope: ScopeId) -> Self::Handle {
        DelegationHandle {
            src_rv: 0,
            dst_rv: 0,
            src_lane: lane.raw() as u16,
            dst_lane: lane.raw() as u16,
            seq_tx: 0,
            seq_rx: 0,
            shard: 0,
            flags: 0,
        }
    }
}
