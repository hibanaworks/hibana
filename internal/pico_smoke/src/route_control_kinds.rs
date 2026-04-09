use hibana::substrate::cap::{
    CapShot, ControlResourceKind, ResourceKind,
    advanced::{
        CAP_HANDLE_LEN, CapError, CapsMask, ControlHandling, ControlMint, ControlScopeKind,
        RouteDecisionHandle, RouteDecisionKind, ScopeId, SessionScopedKind,
    },
};
use hibana::substrate::{Lane, SessionId};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RouteControl<const KIND_LABEL: u8, const ARM: u8>;

impl<const KIND_LABEL: u8, const ARM: u8> ResourceKind for RouteControl<KIND_LABEL, ARM> {
    type Handle = RouteDecisionHandle;
    const TAG: u8 = <RouteDecisionKind as ResourceKind>::TAG;
    const NAME: &'static str = "RouteControl";
    const AUTO_MINT_EXTERNAL: bool = false;

    fn encode_handle(handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN] {
        handle.encode()
    }

    fn decode_handle(data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError> {
        RouteDecisionHandle::decode(data)
    }

    fn zeroize(handle: &mut Self::Handle) {
        *handle = RouteDecisionHandle::default();
    }

    fn caps_mask(_handle: &Self::Handle) -> CapsMask {
        CapsMask::empty()
    }

    fn scope_id(handle: &Self::Handle) -> Option<ScopeId> {
        Some(handle.scope)
    }
}

impl<const KIND_LABEL: u8, const ARM: u8> SessionScopedKind for RouteControl<KIND_LABEL, ARM> {
    fn handle_for_session(_sid: SessionId, _lane: Lane) -> RouteDecisionHandle {
        RouteDecisionHandle::default()
    }

    fn shot() -> CapShot {
        CapShot::One
    }
}

impl<const KIND_LABEL: u8, const ARM: u8> ControlMint for RouteControl<KIND_LABEL, ARM> {
    fn mint_handle(_sid: SessionId, _lane: Lane, scope: ScopeId) -> Self::Handle {
        RouteDecisionHandle { scope, arm: ARM }
    }
}

impl<const KIND_LABEL: u8, const ARM: u8> ControlResourceKind for RouteControl<KIND_LABEL, ARM> {
    const LABEL: u8 = KIND_LABEL;
    const SCOPE: ControlScopeKind = ControlScopeKind::Route;
    const TAP_ID: u16 = <RouteDecisionKind as ControlResourceKind>::TAP_ID;
    const SHOT: CapShot = CapShot::One;
    const HANDLING: ControlHandling = ControlHandling::Canonical;
}
