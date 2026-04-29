use hibana::g;
use hibana::substrate::cap::{GenericCapToken, advanced::LoopContinueKind};

const LOOP_CONTINUE_LOGICAL: u8 = 0xA1;

fn main() {
    let _ = g::send::<
        g::Role<0>,
        g::Role<1>,
        g::Msg<{ LOOP_CONTINUE_LOGICAL }, GenericCapToken<LoopContinueKind>, LoopContinueKind>,
        0,
    >();
}
