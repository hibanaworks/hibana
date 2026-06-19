use hibana::{g, runtime};

fn main() {
    let left = g::send::<0, 1, g::Msg<5, ()>>();
    let right = g::send::<0, 2, g::Msg<5, ()>>();
    let route = g::route(left, right).resolve::<7>();
    let role0: runtime::program::RoleProgram<0> = runtime::program::project(&route);
    let role1: runtime::program::RoleProgram<1> = runtime::program::project(&route);
    let role2: runtime::program::RoleProgram<2> = runtime::program::project(&route);
    core::hint::black_box((role0, role1, role2));
}
