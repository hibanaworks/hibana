use hibana::g;
use hibana::integration::{
    cap::{WireControlKind, GenericCapToken, WireControlEffect},
};

const ZERO_TAG_CONTROL_LOGICAL: u8 = 124;
const ZERO_TAG_CONTROL_TAP_ID: u16 = 0x0374;

struct ZeroTagControlKind;

impl WireControlKind for ZeroTagControlKind {    const TAG: u8 = 0;
    const NAME: &'static str = "ZeroTagControl";
    const TAP_ID: u16 = ZERO_TAG_CONTROL_TAP_ID;
    const EFFECT: WireControlEffect = WireControlEffect::Fence;
}

fn main() {
    let _ = g::send::<
        0,
        1,
        g::Msg<ZERO_TAG_CONTROL_LOGICAL, GenericCapToken<ZeroTagControlKind>>,
        0,
    >();
}
