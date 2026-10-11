use hibana::{g, runtime};

fn main() {
    let program = g::par(
        g::send::<1, 0, g::Msg<5, ()>>(),
        g::send::<2, 0, g::Msg<5, ()>>(),
    );
    let role0: runtime::program::RoleProgram<0> = runtime::program::project(&program);
    let role1: runtime::program::RoleProgram<1> = runtime::program::project(&program);
    let role2: runtime::program::RoleProgram<2> = runtime::program::project(&program);
    core::hint::black_box((role0, role1, role2));
}
