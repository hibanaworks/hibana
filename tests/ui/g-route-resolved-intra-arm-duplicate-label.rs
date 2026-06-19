use hibana::{g, runtime};

fn main() {
    let left = g::par(
        g::send::<0, 1, g::Msg<9, ()>>(),
        g::send::<2, 1, g::Msg<9, ()>>(),
    );
    let right = g::send::<0, 1, g::Msg<10, ()>>();
    let route = g::route(left, right).resolve::<7>();
    let projected: runtime::program::RoleProgram<1> = runtime::program::project(&route);
    core::hint::black_box(projected);
}
