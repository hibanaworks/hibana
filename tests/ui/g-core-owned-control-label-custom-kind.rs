use hibana::g;
use hibana::substrate::{
    cap::{CapShot, ControlResourceKind, GenericCapToken, ResourceKind},
    cap::advanced::{
        CAP_HANDLE_LEN, CapError, ControlOp, ControlPath, ControlScopeKind, RouteDecisionKind,
        ScopeId,
    },
};

struct FakeRouteDecisionKind;

impl ResourceKind for FakeRouteDecisionKind {
    type Handle = ();
    const TAG: u8 = <RouteDecisionKind as ResourceKind>::TAG;
    const NAME: &'static str = "FakeRouteDecision";

    fn encode_handle(_handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN] {
        [0; CAP_HANDLE_LEN]
    }

    fn decode_handle(_data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError> {
        Ok(())
    }

    fn zeroize(_handle: &mut Self::Handle) {}
}

impl ControlResourceKind for FakeRouteDecisionKind {
    const LABEL: u8 = 57;
    const SCOPE: ControlScopeKind = ControlScopeKind::Route;
    const TAP_ID: u16 = <RouteDecisionKind as ControlResourceKind>::TAP_ID;
    const SHOT: CapShot = CapShot::One;
    const PATH: ControlPath = ControlPath::Local;
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
        g::Role<0>,
        g::Msg<
            57,
            GenericCapToken<FakeRouteDecisionKind>,
            FakeRouteDecisionKind,
        >,
        0,
    >();
}
