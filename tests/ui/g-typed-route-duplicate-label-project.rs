use hibana::{g, integration};

fn main() {
    let arm_one = g::send::<0, 0, g::Msg<5, ()>>();
    let arm_two = g::send::<0, 0, g::Msg<5, ()>>();
    let route = g::route(arm_one, arm_two);
    let projected: integration::program::RoleProgram<0> = integration::program::project(&route);
    core::hint::black_box(projected);
}
