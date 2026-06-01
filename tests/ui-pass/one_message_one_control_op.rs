use hibana::g;
use hibana::integration::{
    cap::{WireControlKind, GenericCapToken, WireControlEffect},
};

struct SingleOpKind;

impl WireControlKind for SingleOpKind {    const TAG: u8 = 0x73;
    const EFFECT: WireControlEffect = WireControlEffect::TxAbort;
}

fn main() {
    let _ = g::send::<
        0,
        1,
        g::Msg<123, GenericCapToken<SingleOpKind>>,
        0,
    >();
}
