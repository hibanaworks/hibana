use hibana::substrate::cap::{
    CapShot, ControlResourceKind, ResourceKind,
    advanced::{CAP_HANDLE_LEN, CapError, ControlOp, ControlPath, ControlScopeKind, ScopeId},
};
use hibana::substrate::{Lane, SessionId};

pub(crate) const LABEL_TOPOLOGY_BEGIN_CONTROL: u8 = 121;
pub(crate) const TAG_TOPOLOGY_BEGIN_CONTROL: u8 = 0x71;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TopologyBeginControl;

impl ResourceKind for TopologyBeginControl {
    type Handle = (u32, u32);
    const TAG: u8 = TAG_TOPOLOGY_BEGIN_CONTROL;
    const NAME: &'static str = "TopologyBeginControl";

    fn encode_handle(handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN] {
        let mut buf = [0u8; CAP_HANDLE_LEN];
        buf[0..4].copy_from_slice(&handle.0.to_be_bytes());
        buf[4..8].copy_from_slice(&handle.1.to_be_bytes());
        buf
    }

    fn decode_handle(data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError> {
        Ok((
            u32::from_be_bytes([data[0], data[1], data[2], data[3]]),
            u32::from_be_bytes([data[4], data[5], data[6], data[7]]),
        ))
    }

    fn zeroize(handle: &mut Self::Handle) {
        *handle = (0, 0);
    }
}

impl ControlResourceKind for TopologyBeginControl {
    const LABEL: u8 = LABEL_TOPOLOGY_BEGIN_CONTROL;
    const SCOPE: ControlScopeKind = ControlScopeKind::Splice;
    const TAP_ID: u16 = 0;
    const SHOT: CapShot = CapShot::One;
    const PATH: ControlPath = ControlPath::Wire;
    const OP: ControlOp = ControlOp::TopologyBegin;
    const AUTO_MINT_WIRE: bool = true;

    fn mint_handle(
        _sid: SessionId,
        _lane: Lane,
        _scope: ScopeId,
    ) -> <Self as ResourceKind>::Handle {
        (0, 0)
    }
}
