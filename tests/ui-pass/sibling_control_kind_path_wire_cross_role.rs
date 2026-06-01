use hibana::g;
use hibana::integration::{
    cap::{WireControlKind, GenericCapToken, WireControlEffect},
};

struct WireKind;

impl WireControlKind for WireKind {    const TAG: u8 = 0x72;
    const EFFECT: WireControlEffect = WireControlEffect::TxCommit;
}

fn main() {
    let _ = g::send::<
        0,
        1,
        g::Msg<122, GenericCapToken<WireKind>>,
        0,
    >();
}
