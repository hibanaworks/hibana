use hibana::g::{self, Msg, Role};

const _: () = {
    let left_arm = g::send::<Role<0>, Role<1>, Msg<3, ()>, 0>();
    let right_arm = g::send::<Role<0>, Role<1>, Msg<4, ()>, 0>();
    let _: g::Program<_> = g::route(left_arm, right_arm);
};

fn main() {}
