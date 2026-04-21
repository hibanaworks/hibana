use hibana::g;
use hibana::substrate::cap::{GenericCapToken, advanced::LoopContinueKind};

const LABEL_LOOP_CONTINUE: u8 = 48;

fn main() {
    let _ = g::send::<
        g::Role<0>,
        g::Role<1>,
        g::Msg<
            { LABEL_LOOP_CONTINUE },
            GenericCapToken<LoopContinueKind>,
            LoopContinueKind,
        >,
        0,
    >();
}
