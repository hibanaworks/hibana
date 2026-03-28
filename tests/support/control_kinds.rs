use hibana::substrate::cap::{
    CapShot, ControlResourceKind, ResourceKind,
    advanced::{
        CAP_HANDLE_LEN, CapError, CapsMask, ControlHandling, ControlMint, ControlScopeKind,
        RouteDecisionHandle, RouteDecisionKind, ScopeId, SessionScopedKind,
    },
};
use hibana::substrate::{Lane, SessionId};

const fn control_scope_from_u8(raw: u8) -> ControlScopeKind {
    match raw {
        0 => ControlScopeKind::None,
        1 => ControlScopeKind::Loop,
        2 => ControlScopeKind::Checkpoint,
        3 => ControlScopeKind::Cancel,
        4 => ControlScopeKind::Splice,
        5 => ControlScopeKind::Reroute,
        6 => ControlScopeKind::Policy,
        7 => ControlScopeKind::Route,
        _ => panic!("unsupported control scope"),
    }
}

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnitControl<const TAG: u8, const KIND_LABEL: u8, const SCOPE: u8, const TAP_ID: u16>;

impl<const TAG: u8, const KIND_LABEL: u8, const SCOPE: u8, const TAP_ID: u16> ResourceKind
    for UnitControl<TAG, KIND_LABEL, SCOPE, TAP_ID>
{
    type Handle = ();
    const TAG: u8 = TAG;
    const NAME: &'static str = "UnitControl";
    const AUTO_MINT_EXTERNAL: bool = false;

    fn encode_handle(_handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN] {
        [0u8; CAP_HANDLE_LEN]
    }

    fn decode_handle(_data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError> {
        Ok(())
    }

    fn zeroize(_handle: &mut Self::Handle) {}

    fn caps_mask(_handle: &Self::Handle) -> CapsMask {
        CapsMask::empty()
    }

    fn scope_id(_handle: &Self::Handle) -> Option<ScopeId> {
        None
    }
}

impl<const TAG: u8, const KIND_LABEL: u8, const SCOPE: u8, const TAP_ID: u16> ControlMint
    for UnitControl<TAG, KIND_LABEL, SCOPE, TAP_ID>
{
    fn mint_handle(_sid: SessionId, _lane: Lane, _scope: ScopeId) -> Self::Handle {}
}

impl<const TAG: u8, const KIND_LABEL: u8, const SCOPE: u8, const TAP_ID: u16> ControlResourceKind
    for UnitControl<TAG, KIND_LABEL, SCOPE, TAP_ID>
{
    const LABEL: u8 = KIND_LABEL;
    const SCOPE: ControlScopeKind = control_scope_from_u8(SCOPE);
    const TAP_ID: u16 = TAP_ID;
    const SHOT: CapShot = CapShot::One;
    const HANDLING: ControlHandling = ControlHandling::Canonical;
}
