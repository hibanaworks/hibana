use hibana::g;
use hibana::integration::{
    cap::{CapShot, ControlResourceKind, GenericCapToken, ResourceKind},
    cap::control::{CAP_HANDLE_LEN, CapError, ControlOp, ControlPath, ControlScopeKind},
};

struct LoadBeginKind;

impl ResourceKind for LoadBeginKind {
    type Handle = ();
    const TAG: u8 = 0x50;
    const NAME: &'static str = "LoadBegin";

    fn encode_handle(_handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN] {
        [0; CAP_HANDLE_LEN]
    }

    fn decode_handle(_data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError> {
        Ok(())
    }

    fn zeroize(_handle: &mut Self::Handle) {}
}

impl ControlResourceKind for LoadBeginKind {
    const SCOPE: ControlScopeKind = ControlScopeKind::Policy;
    const PATH: ControlPath = ControlPath::Wire;
    const TAP_ID: u16 = 0x036E;
    const SHOT: CapShot = CapShot::Many;
    const OP: ControlOp = ControlOp::Fence;
}

fn main() {
    let _ = g::send::<
        g::Role<0>,
        g::Role<0>,
        g::Msg<110, GenericCapToken<LoadBeginKind>, LoadBeginKind>,
        0,
    >();
}
