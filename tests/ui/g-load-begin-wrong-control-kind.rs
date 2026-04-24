use hibana::g;
use hibana::substrate::{
    cap::{CapShot, ControlResourceKind, GenericCapToken, ResourceKind},
    cap::advanced::{
        CAP_HANDLE_LEN, CapError, ControlPath, ControlOp, ControlScopeKind, ScopeId,
    },
};

const LABEL_MGMT_LOAD_BEGIN: u8 = 110;

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
    const LABEL: u8 = LABEL_MGMT_LOAD_BEGIN;
    const SCOPE: ControlScopeKind = ControlScopeKind::Policy;
    const TAP_ID: u16 = 0x0300 + LABEL_MGMT_LOAD_BEGIN as u16;
    const SHOT: CapShot = CapShot::One;
    const PATH: ControlPath = ControlPath::Wire;
    const OP: ControlOp = ControlOp::Fence;
    const AUTO_MINT_WIRE: bool = true;

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
        g::Role<0>,
        g::Msg<
            { LABEL_MGMT_LOAD_BEGIN },
            GenericCapToken<LoadBeginKind>,
            LoadBeginKind,
        >,
        0,
    >();
}
