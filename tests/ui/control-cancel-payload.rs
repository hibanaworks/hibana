use hibana::g::{self};

const LABEL_CANCEL: u8 = 60;

fn main() {
    let bad = g::send::<g::Role<0>, g::Role<1>, g::Msg<LABEL_CANCEL, ()>, 0>();
    let _ = bad;
}
