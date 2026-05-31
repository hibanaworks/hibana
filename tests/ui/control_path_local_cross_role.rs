use hibana::g;
use hibana::integration::cap::{control::LoopContinueKind};

const LOOP_CONTINUE_LOGICAL: u8 = 0xA1;

fn main() {
    let _ = g::send::<
        0,
        1,
        g::Msg<{ LOOP_CONTINUE_LOGICAL }, (), LoopContinueKind>,
        0,
    >();
}
