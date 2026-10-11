use hibana::{g, runtime};

fn main() {
    let protocol = g::seq(
        g::send::<0, 2, g::Msg<1, ()>>(),
        g::send::<1, 2, g::Msg<2, ()>>(),
    );
    let _: runtime::program::RoleProgram<2> = runtime::program::project(&protocol);
}
