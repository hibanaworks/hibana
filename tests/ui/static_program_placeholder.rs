use hibana::g::{self};

static PROGRAM: g::Program<_> =
    g::send::<g::Role<0>, g::Role<1>, g::Msg<7, u16>, 0>();

fn main() {
    let _ = &PROGRAM;
}
