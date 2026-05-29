use hibana::g;
use hibana::integration::{
    cap::{CapShot, ControlResourceKind, GenericCapToken, ResourceKind},
    cap::control::{
        CAP_HANDLE_LEN, CapError, ControlOp, ControlPath, ControlScopeKind, ScopeId,
    },
};

struct AutoMintWireKind;

impl ResourceKind for AutoMintWireKind {
    type Handle = ();
    const TAG: u8 = 0x72;
    const NAME: &'static str = "AutoMintWireKind";

    fn encode_handle(_handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN] {
        [0; CAP_HANDLE_LEN]
    }

    fn decode_handle(_data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError> {
        Ok(())
    }

    fn zeroize(_handle: &mut Self::Handle) {}
}

impl ControlResourceKind for AutoMintWireKind {
    const SCOPE: ControlScopeKind = ControlScopeKind::Policy;
    const PATH: ControlPath = ControlPath::Wire;
    const TAP_ID: u16 = 0x0472;
    const SHOT: CapShot = CapShot::Many;
    const OP: ControlOp = ControlOp::Fence;
    const AUTO_MINT_WIRE: bool = true;

    fn mint_handle(
        _session: hibana::integration::ids::SessionId,
        _lane: hibana::integration::ids::Lane,
        _scope: ScopeId,
    ) -> Self::Handle {
    }
}

fn main() {
    let _ = g::send::<
        g::Role<0>,
        g::Role<1>,
        g::Msg<122, GenericCapToken<AutoMintWireKind>, AutoMintWireKind>,
        0,
    >();
}
