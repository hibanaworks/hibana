use hibana::g;
use hibana::substrate::{
    cap::{CapShot, ControlResourceKind, GenericCapToken, ResourceKind},
    cap::advanced::{
        CAP_HANDLE_LEN, CapError, ControlOp, ControlPath, ControlScopeKind, ScopeId,
    },
};

const LOOP_CONTROL_LOGICAL: u8 = 125;
const LOOP_CONTROL_TAP_ID: u16 = 0x0375;

struct WireLoopControlKind;

impl ResourceKind for WireLoopControlKind {
    type Handle = ();
    const TAG: u8 = 0x75;
    const NAME: &'static str = "WireLoopControl";

    fn encode_handle(_handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN] {
        [0; CAP_HANDLE_LEN]
    }

    fn decode_handle(_data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError> {
        Ok(())
    }

    fn zeroize(_handle: &mut Self::Handle) {}
}

impl ControlResourceKind for WireLoopControlKind {
    const SCOPE: ControlScopeKind = ControlScopeKind::Loop;
    const TAP_ID: u16 = LOOP_CONTROL_TAP_ID;
    const SHOT: CapShot = CapShot::One;
    const PATH: ControlPath = ControlPath::Wire;
    const OP: ControlOp = ControlOp::LoopContinue;
    const AUTO_MINT_WIRE: bool = false;

    fn mint_handle(
        _sid: hibana::substrate::ids::SessionId,
        _lane: hibana::substrate::ids::Lane,
        _scope: ScopeId,
    ) -> <Self as ResourceKind>::Handle {
    }
}

fn main() {
    let _ = g::send::<
        g::Role<0>,
        g::Role<1>,
        g::Msg<
            { LOOP_CONTROL_LOGICAL },
            GenericCapToken<WireLoopControlKind>,
            WireLoopControlKind,
        >,
        0,
    >();
}
