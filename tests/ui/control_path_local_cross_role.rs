use hibana::g;
use hibana::substrate::cap::{GenericCapToken, advanced::LoopContinueKind};

fn main() {
    let _ = g::send::<
        g::Role<0>,
        g::Role<1>,
        g::Msg<48, GenericCapToken<LoopContinueKind>, LoopContinueKind>,
        0,
    >();
}
