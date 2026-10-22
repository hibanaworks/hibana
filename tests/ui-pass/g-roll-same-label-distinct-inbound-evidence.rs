use hibana::{g, runtime};

fn main() {
    let body = g::send::<1, 0, g::Msg<5, ()>>().roll();
    let protocol = g::seq(body, g::send::<2, 0, g::Msg<5, ()>>());
    let _: runtime::program::RoleProgram<0> = runtime::program::project(&protocol);
}
