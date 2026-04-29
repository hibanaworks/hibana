use hibana::g;
use hibana::substrate::program::{RoleProgram, project};

fn main() {
    let program = g::send::<g::Role<0>, g::Role<1>, g::Msg<1, u8>, 0>();
    let program = g::seq(program, g::send::<g::Role<1>, g::Role<0>, g::Msg<2, u8>, 0>());
    let program = g::seq(program, g::send::<g::Role<0>, g::Role<1>, g::Msg<3, u8>, 0>());
    let program = g::seq(program, g::send::<g::Role<1>, g::Role<0>, g::Msg<4, u8>, 0>());
    let program = g::seq(program, g::send::<g::Role<0>, g::Role<1>, g::Msg<5, u8>, 0>());
    let program = g::seq(program, g::send::<g::Role<1>, g::Role<0>, g::Msg<6, u8>, 0>());
    let program = g::seq(program, g::send::<g::Role<0>, g::Role<1>, g::Msg<7, u8>, 0>());
    let program = g::seq(program, g::send::<g::Role<1>, g::Role<0>, g::Msg<8, u8>, 0>());
    let program = g::seq(program, g::send::<g::Role<0>, g::Role<1>, g::Msg<9, u8>, 0>());
    let program = g::seq(program, g::send::<g::Role<1>, g::Role<0>, g::Msg<10, u8>, 0>());
    let program = g::seq(program, g::send::<g::Role<0>, g::Role<1>, g::Msg<11, u8>, 0>());
    let program = g::seq(program, g::send::<g::Role<1>, g::Role<0>, g::Msg<12, u8>, 0>());
    let program = g::seq(program, g::send::<g::Role<0>, g::Role<1>, g::Msg<13, u8>, 0>());
    let program = g::seq(program, g::send::<g::Role<1>, g::Role<0>, g::Msg<14, u8>, 0>());
    let program = g::seq(program, g::send::<g::Role<0>, g::Role<1>, g::Msg<15, u8>, 0>());
    let program = g::seq(program, g::send::<g::Role<1>, g::Role<0>, g::Msg<16, u8>, 0>());
    let program = g::seq(program, g::send::<g::Role<0>, g::Role<1>, g::Msg<17, u8>, 0>());
    let program = g::seq(program, g::send::<g::Role<1>, g::Role<0>, g::Msg<18, u8>, 0>());
    let program = g::seq(program, g::send::<g::Role<0>, g::Role<1>, g::Msg<19, u8>, 0>());
    let program = g::seq(program, g::send::<g::Role<1>, g::Role<0>, g::Msg<20, u8>, 0>());
    let program = g::seq(program, g::send::<g::Role<0>, g::Role<1>, g::Msg<21, u8>, 0>());
    let program = g::seq(program, g::send::<g::Role<1>, g::Role<0>, g::Msg<22, u8>, 0>());
    let program = g::seq(program, g::send::<g::Role<0>, g::Role<1>, g::Msg<23, u8>, 0>());
    let program = g::seq(program, g::send::<g::Role<1>, g::Role<0>, g::Msg<24, u8>, 0>());

    let client: RoleProgram<0> = project(&program);
    let server: RoleProgram<1> = project(&program);
    let endpoints = (client, server);
    core::hint::black_box(endpoints);
}
