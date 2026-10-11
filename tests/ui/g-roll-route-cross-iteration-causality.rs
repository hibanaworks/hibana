use hibana::{g, runtime};

fn main() {
    let protocol = g::route(
        g::send::<0, 2, g::Msg<1, ()>>(),
        g::send::<1, 2, g::Msg<2, ()>>(),
    )
    .resolve::<7>()
    .roll();
    let _: runtime::program::RoleProgram<2> = runtime::program::project(&protocol);
}
