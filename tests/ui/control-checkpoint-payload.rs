use hibana::g::{self};

const LABEL_CHECKPOINT: u8 = 61;

fn main() {
    let bad = g::send::<g::Role<0>, g::Role<1>, g::Msg<LABEL_CHECKPOINT, ()>, 0>();
    let _ = bad;
}
