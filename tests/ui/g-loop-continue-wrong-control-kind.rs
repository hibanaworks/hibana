use hibana::g;
use hibana::integration::cap::{control::LoopContinueKind};

const TEST_LOOP_CONTINUE_LOGICAL: u8 = 0xA1;

fn main() {
    let _ = g::send::<
        g::Role<0>,
        g::Role<1>,
        g::Msg<
            { TEST_LOOP_CONTINUE_LOGICAL },
            (),
            LoopContinueKind,
        >,
        0,
    >();
}
