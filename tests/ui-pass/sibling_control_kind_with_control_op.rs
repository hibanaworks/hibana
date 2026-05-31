use hibana::g;
use hibana::integration::{
    cap::{WireControlKind, GenericCapToken, WireControlEffect},
};

struct FenceKind;

impl WireControlKind for FenceKind {    const TAG: u8 = 0x70;
    const TAP_ID: u16 = 0x0470;
    const EFFECT: WireControlEffect = WireControlEffect::Fence;
}

fn main() {
    let _ = g::send::<
        0,
        1,
        g::Msg<120, GenericCapToken<FenceKind>>,
        0,
    >();
}
