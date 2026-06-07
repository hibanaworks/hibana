use hibana::g::{self};

const PROGRAM: g::Program<_> =
    g::send::<0, 1, g::Msg<7, u16>>();

fn main() {
    let _ = PROGRAM;
}
