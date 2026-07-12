use hibana::{g, runtime};

fn main() {
    let branch = g::route(
        g::send::<2, 1, g::Msg<2, ()>>(),
        g::send::<3, 4, g::Msg<4, ()>>(),
    )
    .resolve::<7>();
    let protocol = g::seq(
        g::send::<0, 2, g::Msg<1, ()>>(),
        g::seq(branch, g::send::<1, 2, g::Msg<3, ()>>()),
    );
    let _: runtime::program::RoleProgram<2> = runtime::program::project(&protocol);
}
