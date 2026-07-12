use hibana::{g, runtime};

fn main() {
    let left = g::seq(
        g::send::<0, 1, g::Msg<1, ()>>(),
        g::send::<1, 3, g::Msg<5, ()>>(),
    );
    let right = g::seq(
        g::send::<0, 1, g::Msg<2, ()>>(),
        g::send::<1, 3, g::Msg<5, ()>>(),
    );
    let route = g::route(left, right);
    let projected: runtime::program::RoleProgram<3> = runtime::program::project(&route);
    core::hint::black_box(projected);
}
