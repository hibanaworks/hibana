use hibana::{g, runtime::program::project};

fn main() {
    let protocol = g::seq(
        g::send::<0, 1, g::Msg<3, ()>>().roll(),
        g::seq(
            g::send::<1, 0, g::Msg<24, ()>>(),
            g::send::<0, 1, g::Msg<3, ()>>(),
        ),
    );
    let _ = project::<0>(&protocol);
}
