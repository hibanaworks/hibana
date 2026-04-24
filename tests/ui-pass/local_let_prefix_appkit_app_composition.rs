use hibana::substrate::program::{RoleProgram, project};
use hibana::g::{self};

fn main() {
    let transport_prefix = || {
        g::seq(
            g::send::<g::Role<0>, g::Role<1>, g::Msg<1, ()>, 0>(),
            g::send::<g::Role<1>, g::Role<0>, g::Msg<2, ()>, 0>(),
        )
    };
    let appkit_prefix =
        || g::send::<g::Role<0>, g::Role<1>, g::Msg<3, ()>, 0>();
    let app = || {
        g::seq(
            g::send::<g::Role<0>, g::Role<1>, g::Msg<10, u32>, 0>(),
            g::send::<g::Role<1>, g::Role<0>, g::Msg<11, u32>, 0>(),
        )
    };
    let program = g::seq(transport_prefix(), g::seq(appkit_prefix(), app()));
    let projected: RoleProgram<0> = project(&program);
    let _ = projected;
}
