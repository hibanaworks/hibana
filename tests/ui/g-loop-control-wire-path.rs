use hibana::g;
use hibana::substrate::{
    cap::{CapShot, ControlResourceKind, GenericCapToken, ResourceKind},
    cap::advanced::{
        CAP_HANDLE_LEN, CapError, ControlOp, ControlPath, ControlScopeKind, ScopeId,
    },
};

const LABEL_LOOP_CONTROL: u8 = 125;

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
    const LABEL: u8 = LABEL_LOOP_CONTROL;
    const SCOPE: ControlScopeKind = ControlScopeKind::Loop;
    const TAP_ID: u16 = 0x0300 + LABEL_LOOP_CONTROL as u16;
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
            { LABEL_LOOP_CONTROL },
            GenericCapToken<WireLoopControlKind>,
            WireLoopControlKind,
        >,
        0,
    >();
}
