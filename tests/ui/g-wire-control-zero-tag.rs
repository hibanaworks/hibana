use hibana::g;
use hibana::integration::{
    cap::{WireControlKind, GenericCapToken, WireControlEffect},
};

const ZERO_TAG_CONTROL_LOGICAL: u8 = 124;

struct ZeroTagControlKind;

impl WireControlKind for ZeroTagControlKind {    const TAG: u8 = 0;
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
