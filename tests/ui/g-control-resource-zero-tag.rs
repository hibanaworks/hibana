use hibana::g;
use hibana::integration::{
    cap::{CapShot, ControlResourceKind, GenericCapToken, ResourceKind},
    cap::control::{CAP_HANDLE_LEN, CapError, ControlOp, ControlPath, ControlScopeKind},
};

const ZERO_TAG_CONTROL_LOGICAL: u8 = 124;
const ZERO_TAG_CONTROL_TAP_ID: u16 = 0x0374;

struct ZeroTagControlKind;

impl ResourceKind for ZeroTagControlKind {
    type Handle = ();
    const TAG: u8 = 0;
    const NAME: &'static str = "ZeroTagControl";

    fn encode_handle(_handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN] {
        [0; CAP_HANDLE_LEN]
    }

    fn decode_handle(_data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError> {
        Ok(())
    }

    fn zeroize(_handle: &mut Self::Handle) {}
}

impl ControlResourceKind for ZeroTagControlKind {
    const SCOPE: ControlScopeKind = ControlScopeKind::None;
    const TAP_ID: u16 = ZERO_TAG_CONTROL_TAP_ID;
    const SHOT: CapShot = CapShot::Many;
    const PATH: ControlPath = ControlPath::Wire;
    const OP: ControlOp = ControlOp::Fence;
}

fn main() {
    let _ = g::send::<
        g::Role<0>,
        g::Role<1>,
        g::Msg<ZERO_TAG_CONTROL_LOGICAL, GenericCapToken<ZeroTagControlKind>, ZeroTagControlKind>,
        0,
    >();
}
