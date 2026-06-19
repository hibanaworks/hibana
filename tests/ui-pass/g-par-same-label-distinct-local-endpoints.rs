use hibana::{g, runtime};

fn main() {
    let program = g::par(
        g::send::<0, 1, g::Msg<5, ()>>(),
        g::send::<2, 3, g::Msg<5, ()>>(),
    );
    let role0: runtime::program::RoleProgram<0> = runtime::program::project(&program);
    let role1: runtime::program::RoleProgram<1> = runtime::program::project(&program);
    let role2: runtime::program::RoleProgram<2> = runtime::program::project(&program);
    let role3: runtime::program::RoleProgram<3> = runtime::program::project(&program);
    core::hint::black_box((role0, role1, role2, role3));
}
