use hibana::substrate::cap::{
    CapShot, ControlResourceKind, ResourceKind,
    advanced::{
        CAP_HANDLE_LEN, CapError, ControlOp, ControlPath, ControlScopeKind, RouteDecisionKind,
        ScopeId,
    },
};

type RouteWireHandle = (u8, u64);

fn encode_route_handle(handle: RouteWireHandle) -> [u8; CAP_HANDLE_LEN] {
    let mut buf = [0u8; CAP_HANDLE_LEN];
    buf[0] = handle.0;
    buf[1..9].copy_from_slice(&handle.1.to_le_bytes());
    buf
}

fn decode_route_handle(data: [u8; CAP_HANDLE_LEN]) -> RouteWireHandle {
    let mut scope_bytes = [0u8; 8];
    scope_bytes.copy_from_slice(&data[1..9]);
    (data[0], u64::from_le_bytes(scope_bytes))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RouteControl<const KIND_LABEL: u8, const ARM: u8>;

impl<const KIND_LABEL: u8, const ARM: u8> ResourceKind for RouteControl<KIND_LABEL, ARM> {
    type Handle = RouteWireHandle;
    const TAG: u8 = <RouteDecisionKind as ResourceKind>::TAG;
    const NAME: &'static str = "RouteControl";

    fn encode_handle(handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN] {
        encode_route_handle(*handle)
    }

    fn decode_handle(data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError> {
        Ok(decode_route_handle(data))
    }

    fn zeroize(handle: &mut Self::Handle) {
        *handle = (0, 0);
    }
}

impl<const KIND_LABEL: u8, const ARM: u8> ControlResourceKind for RouteControl<KIND_LABEL, ARM> {
    const LABEL: u8 = KIND_LABEL;
    const SCOPE: ControlScopeKind = ControlScopeKind::Route;
    const TAP_ID: u16 = <RouteDecisionKind as ControlResourceKind>::TAP_ID;
    const SHOT: CapShot = CapShot::One;
    const PATH: ControlPath = ControlPath::Local;
    const OP: ControlOp = ControlOp::RouteDecision;
    const AUTO_MINT_WIRE: bool = false;

    fn mint_handle(
        _sid: hibana::substrate::ids::SessionId,
        _lane: hibana::substrate::ids::Lane,
        scope: ScopeId,
    ) -> <Self as ResourceKind>::Handle {
        (ARM, scope.raw())
    }
}
