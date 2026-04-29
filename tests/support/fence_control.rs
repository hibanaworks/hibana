use crate::control::cap::mint::{
    CAP_HANDLE_LEN, CapError, CapShot, ControlOp, ControlPath, ControlResourceKind, ResourceKind,
};
use crate::control::types::{Lane, SessionId};
use crate::global::const_dsl::{ControlScopeKind, ScopeId};

pub(crate) const FENCE_CONTROL_LOGICAL: u8 = 126;
pub(crate) const TAG_FENCE_CONTROL: u8 = 0x73;
const TAP_FENCE_CONTROL: u16 = 0x037e;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FenceControl;

impl ResourceKind for FenceControl {
    type Handle = ();
    const TAG: u8 = TAG_FENCE_CONTROL;
    const NAME: &'static str = "FenceControl";

    fn encode_handle(_handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN] {
        [0; CAP_HANDLE_LEN]
    }

    fn decode_handle(_data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError> {
        Ok(())
    }

    fn zeroize(_handle: &mut Self::Handle) {}
}

impl ControlResourceKind for FenceControl {
    const SCOPE: ControlScopeKind = ControlScopeKind::Policy;
    const TAP_ID: u16 = TAP_FENCE_CONTROL;
    const SHOT: CapShot = CapShot::One;
    const PATH: ControlPath = ControlPath::Local;
    const OP: ControlOp = ControlOp::Fence;
    const AUTO_MINT_WIRE: bool = false;

    fn mint_handle(
        _sid: SessionId,
        _lane: Lane,
        _scope: ScopeId,
    ) -> <Self as ResourceKind>::Handle {
    }
}
