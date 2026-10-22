use hibana::{g, runtime};

fn main() {
    let protocol = g::seq(
        g::par(
            g::send::<0, 2, g::Msg<1, ()>>(),
            g::send::<2, 1, g::Msg<2, ()>>(),
        ),
        g::send::<1, 2, g::Msg<3, ()>>(),
    );
    let _: runtime::program::RoleProgram<2> = runtime::program::project(&protocol);
}
