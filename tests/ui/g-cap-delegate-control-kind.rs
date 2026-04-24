use hibana::g;
use hibana::substrate::{
    cap::{CapShot, ControlResourceKind, GenericCapToken, ResourceKind},
    cap::advanced::{
        CAP_HANDLE_LEN, CapError, ControlOp, ControlPath, ControlScopeKind, ScopeId,
    },
};

const LABEL_CAP_DELEGATE: u8 = 126;

struct CapDelegateKind;

impl ResourceKind for CapDelegateKind {
    type Handle = ();
    const TAG: u8 = 0x76;
    const NAME: &'static str = "CapDelegateKind";

    fn encode_handle(_handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN] {
        [0; CAP_HANDLE_LEN]
    }

    fn decode_handle(_data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError> {
        Ok(())
    }

    fn zeroize(_handle: &mut Self::Handle) {}
}

impl ControlResourceKind for CapDelegateKind {
    const LABEL: u8 = LABEL_CAP_DELEGATE;
    const SCOPE: ControlScopeKind = ControlScopeKind::Delegate;
    const TAP_ID: u16 = 0x0300 + LABEL_CAP_DELEGATE as u16;
    const SHOT: CapShot = CapShot::One;
    const PATH: ControlPath = ControlPath::Wire;
    const OP: ControlOp = ControlOp::CapDelegate;
    const AUTO_MINT_WIRE: bool = true;

    fn mint_handle(
        _sid: hibana::substrate::SessionId,
        _lane: hibana::substrate::Lane,
        _scope: ScopeId,
    ) -> <Self as ResourceKind>::Handle {
    }
}

fn main() {
    let _ = g::send::<
        g::Role<0>,
        g::Role<1>,
        g::Msg<126, GenericCapToken<CapDelegateKind>, CapDelegateKind>,
        0,
    >();
}
