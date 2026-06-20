use hibana::{g, runtime};

fn main() {
    let body = g::send::<0, 1, g::Msg<5, ()>>().roll();
    let program = g::seq(body, g::send::<0, 2, g::Msg<5, ()>>());
    let projected: runtime::program::RoleProgram<0> = runtime::program::project(&program);
    core::hint::black_box(projected);
}
