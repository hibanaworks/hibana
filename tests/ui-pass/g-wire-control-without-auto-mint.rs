use hibana::g;
use hibana::substrate::{
    cap::{CapShot, ControlResourceKind, GenericCapToken, ResourceKind},
    cap::advanced::{
        CAP_HANDLE_LEN, CapError, ControlOp, ControlPath, ControlScopeKind, ScopeId,
    },
};

struct ManualWireControl;

impl ResourceKind for ManualWireControl {
    type Handle = ();
    const TAG: u8 = 0x74;
    const NAME: &'static str = "ManualWireControl";

    fn encode_handle(_handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN] {
        [0; CAP_HANDLE_LEN]
    }

    fn decode_handle(_data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError> {
        Ok(())
    }

    fn zeroize(_handle: &mut Self::Handle) {}
}

impl ControlResourceKind for ManualWireControl {
    const LABEL: u8 = 124;
    const SCOPE: ControlScopeKind = ControlScopeKind::Policy;
    const PATH: ControlPath = ControlPath::Wire;
    const TAP_ID: u16 = 0x0474;
    const SHOT: CapShot = CapShot::One;
    const OP: ControlOp = ControlOp::Fence;
    const AUTO_MINT_WIRE: bool = false;

    fn mint_handle(
        _session: hibana::substrate::SessionId,
        _lane: hibana::substrate::Lane,
        _scope: ScopeId,
    ) -> Self::Handle {
    }
}

fn main() {
    let _ = g::send::<
        g::Role<0>,
        g::Role<1>,
        g::Msg<124, GenericCapToken<ManualWireControl>, ManualWireControl>,
        0,
    >();
}
