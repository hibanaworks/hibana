use hibana::g;
use hibana::substrate::{
    cap::{CapShot, ControlResourceKind, GenericCapToken, ResourceKind},
    cap::advanced::{
        CAP_HANDLE_LEN, CapError, ControlOp, ControlPath, ControlScopeKind, ScopeId,
    },
};

struct LocalKind;

impl ResourceKind for LocalKind {
    type Handle = ();
    const TAG: u8 = 0x71;
    const NAME: &'static str = "LocalKind";

    fn encode_handle(_handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN] {
        [0; CAP_HANDLE_LEN]
    }

    fn decode_handle(_data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError> {
        Ok(())
    }

    fn zeroize(_handle: &mut Self::Handle) {}
}

impl ControlResourceKind for LocalKind {
    const LABEL: u8 = 121;
    const SCOPE: ControlScopeKind = ControlScopeKind::Policy;
    const PATH: ControlPath = ControlPath::Local;
    const TAP_ID: u16 = 0x0471;
    const SHOT: CapShot = CapShot::One;
    const OP: ControlOp = ControlOp::Fence;
    const AUTO_MINT_WIRE: bool = false;

    fn mint_handle(
        _session: hibana::substrate::ids::SessionId,
        _lane: hibana::substrate::ids::Lane,
        _scope: ScopeId,
    ) -> Self::Handle {
    }
}

fn main() {
    let _ = g::send::<
        g::Role<0>,
        g::Role<0>,
        g::Msg<121, GenericCapToken<LocalKind>, LocalKind>,
        0,
    >();
}
