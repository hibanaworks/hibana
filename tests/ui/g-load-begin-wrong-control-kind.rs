use hibana::g;
use hibana::g::advanced::CanonicalControl;
use hibana::substrate::{
    cap::{CapShot, ControlResourceKind, GenericCapToken, ResourceKind},
    cap::advanced::{CAP_HANDLE_LEN, CapError, CapsMask, ControlHandling, ControlScopeKind, ScopeId},
};

const LABEL_MGMT_LOAD_BEGIN: u8 = 40;

struct LoadBeginKind;

impl ResourceKind for LoadBeginKind {
    type Handle = ();
    const TAG: u8 = 0x50;
    const NAME: &'static str = "LoadBegin";
    const AUTO_MINT_EXTERNAL: bool = false;

    fn encode_handle(_handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN] {
        [0; CAP_HANDLE_LEN]
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

impl ControlResourceKind for LoadBeginKind {
    const LABEL: u8 = LABEL_MGMT_LOAD_BEGIN;
    const SCOPE: ControlScopeKind = ControlScopeKind::Policy;
    const TAP_ID: u16 = 0;
    const SHOT: CapShot = CapShot::One;
    const HANDLING: ControlHandling = ControlHandling::External;
}

fn main() {
    let _ = g::send::<
        g::Role<0>,
        g::Role<1>,
        g::Msg<
            { LABEL_MGMT_LOAD_BEGIN },
            GenericCapToken<LoadBeginKind>,
            CanonicalControl<LoadBeginKind>,
        >,
        0,
    >();
}
