use hibana::g::{self, Msg, Role, advanced::project};
use hibana::substrate::{
    cap::{CapShot, ControlResourceKind, GenericCapToken, ResourceKind},
    cap::advanced::{CAP_HANDLE_LEN, CapError, ControlOp, ControlPath, ControlScopeKind, ScopeId},
};

const LABEL_POLICY_ANNOTATE: u8 = 124;

struct PolicyAnnotateKind;

impl ResourceKind for PolicyAnnotateKind {
    type Handle = ();
    const TAG: u8 = 0x74;
    const NAME: &'static str = "PolicyAnnotate";

    fn encode_handle(_handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN] {
        [0; CAP_HANDLE_LEN]
    }

    fn decode_handle(_data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError> {
        Ok(())
    }

    fn zeroize(_handle: &mut Self::Handle) {}
}

impl ControlResourceKind for PolicyAnnotateKind {
    const LABEL: u8 = LABEL_POLICY_ANNOTATE;
    const SCOPE: ControlScopeKind = ControlScopeKind::Policy;
    const TAP_ID: u16 = 0x0300 + LABEL_POLICY_ANNOTATE as u16;
    const SHOT: CapShot = CapShot::One;
    const PATH: ControlPath = ControlPath::Local;
    const OP: ControlOp = ControlOp::Fence;
    const AUTO_MINT_WIRE: bool = false;

    fn mint_handle(
        _sid: hibana::substrate::SessionId,
        _lane: hibana::substrate::Lane,
        _scope: ScopeId,
    ) -> <Self as ResourceKind>::Handle {
    }
}

fn main() {
    let program = g::send::<
        Role<0>,
        Role<0>,
        Msg<
            { LABEL_POLICY_ANNOTATE },
            GenericCapToken<PolicyAnnotateKind>,
            PolicyAnnotateKind,
        >,
        0,
    >()
    .policy::<7>();
    let _: hibana::g::advanced::RoleProgram<0> = project(&program);
}
