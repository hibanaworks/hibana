use hibana::g;
use hibana::integration::{
    cap::{WireControlKind, GenericCapToken, WireControlEffect},
};

const MGMT_LOAD_BEGIN_LOGICAL: u8 = 110;
const MGMT_LOAD_BEGIN_TAP_ID: u16 = 0x0350;

struct LoadBeginKind;

impl WireControlKind for LoadBeginKind {    const TAG: u8 = 0x50;
    const TAP_ID: u16 = MGMT_LOAD_BEGIN_TAP_ID;
    const EFFECT: WireControlEffect = WireControlEffect::Fence;
}

fn main() {
    let _ = g::send::<
        0,
        0,
        g::Msg<{ MGMT_LOAD_BEGIN_LOGICAL }, GenericCapToken<LoadBeginKind>>,
        0,
    >();
}
