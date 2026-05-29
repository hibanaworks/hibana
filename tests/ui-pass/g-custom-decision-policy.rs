use hibana::g;
use hibana::integration::{
    cap::{CapShot, ControlResourceKind, ResourceKind},
    cap::control::{CAP_HANDLE_LEN, CapError, ControlOp, ControlPath, ControlScopeKind, RouteArmHandle, ScopeId},
    program::{RoleProgram, project},
};

struct ProtocolRouteDecision;

impl ResourceKind for ProtocolRouteDecision {
    type Handle = RouteArmHandle;
    const TAG: u8 = 0x75;
    const NAME: &'static str = "ProtocolRouteDecision";

    fn encode_handle(handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN] {
        handle.encode()
    }

    fn decode_handle(data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError> {
        RouteArmHandle::decode(data)
    }

    fn zeroize(_handle: &mut Self::Handle) {}
}

impl ControlResourceKind for ProtocolRouteDecision {
    const SCOPE: ControlScopeKind = ControlScopeKind::Route;
    const PATH: ControlPath = ControlPath::Local;
    const TAP_ID: u16 = 0x0475;
    const SHOT: CapShot = CapShot::One;
    const OP: ControlOp = ControlOp::RouteDecision;
    const AUTO_MINT_WIRE: bool = false;

    fn mint_handle(
        _session: hibana::integration::ids::SessionId,
        _lane: hibana::integration::ids::Lane,
        scope: ScopeId,
    ) -> Self::Handle {
        let _ = scope;
        RouteArmHandle::new(0).expect("binary route arm")
    }
}

fn main() {
    let left = g::seq(
        g::send::<g::Role<0>, g::Role<0>, g::Msg<121, (), ProtocolRouteDecision>, 0>()
            .policy::<77>(),
        g::send::<g::Role<0>, g::Role<1>, g::Msg<122, u8>, 0>(),
    );
    let right = g::seq(
        g::send::<g::Role<0>, g::Role<0>, g::Msg<123, (), ProtocolRouteDecision>, 0>()
            .policy::<77>(),
        g::send::<g::Role<0>, g::Role<1>, g::Msg<124, u8>, 0>(),
    );
    let program = g::route(left, right);
    let _: RoleProgram<0> = project(&program);
    let _: RoleProgram<1> = project(&program);
}
