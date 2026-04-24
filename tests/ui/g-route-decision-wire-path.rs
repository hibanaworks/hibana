use hibana::g;
use hibana::substrate::{
    cap::{CapShot, ControlResourceKind, GenericCapToken, ResourceKind},
    cap::advanced::{
        CAP_HANDLE_LEN, CapError, ControlOp, ControlPath, ControlScopeKind, ScopeId,
    },
};

const LABEL_ROUTE_PICK: u8 = 124;

struct WireRouteDecisionKind;

impl ResourceKind for WireRouteDecisionKind {
    type Handle = ();
    const TAG: u8 = 0x74;
    const NAME: &'static str = "WireRouteDecision";

    fn encode_handle(_handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN] {
        [0; CAP_HANDLE_LEN]
    }

    fn decode_handle(_data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError> {
        Ok(())
    }

    fn zeroize(_handle: &mut Self::Handle) {}
}

impl ControlResourceKind for WireRouteDecisionKind {
    const LABEL: u8 = LABEL_ROUTE_PICK;
    const SCOPE: ControlScopeKind = ControlScopeKind::Route;
    const TAP_ID: u16 = 0x0300 + LABEL_ROUTE_PICK as u16;
    const SHOT: CapShot = CapShot::One;
    const PATH: ControlPath = ControlPath::Wire;
    const OP: ControlOp = ControlOp::RouteDecision;
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
            { LABEL_ROUTE_PICK },
            GenericCapToken<WireRouteDecisionKind>,
            WireRouteDecisionKind,
        >,
        0,
    >();
}
