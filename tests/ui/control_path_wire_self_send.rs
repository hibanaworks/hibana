use hibana::g;
use hibana::integration::{
    cap::{WireControlKind, GenericCapToken, WireControlEffect},
};

struct LoadBeginKind;

impl WireControlKind for LoadBeginKind {    const TAG: u8 = 0x50;
    const EFFECT: WireControlEffect = WireControlEffect::Fence;
}

fn main() {
    let _ = g::send::<
        0,
        0,
        g::Msg<110, GenericCapToken<LoadBeginKind>>,
        0,
    >();
}
