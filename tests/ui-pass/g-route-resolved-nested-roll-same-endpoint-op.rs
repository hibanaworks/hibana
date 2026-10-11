use hibana::{g, runtime};

fn main() {
    let inner = g::route(
        g::send::<0, 1, g::Msg<5, ()>>(),
        g::send::<0, 1, g::Msg<5, ()>>(),
    )
    .resolve::<8>();
    let outer = g::route(
        g::send::<0, 1, g::Msg<6, ()>>(),
        g::seq(g::send::<0, 1, g::Msg<7, ()>>(), inner),
    )
    .resolve::<7>()
    .roll();
    let role0: runtime::program::RoleProgram<0> = runtime::program::project(&outer);
    let role1: runtime::program::RoleProgram<1> = runtime::program::project(&outer);
    core::hint::black_box((role0, role1));
}
