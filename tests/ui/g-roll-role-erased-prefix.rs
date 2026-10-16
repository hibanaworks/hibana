use hibana::{g, runtime::program::project};

fn main() {
    let protocol = g::seq(
        g::seq(
            g::send::<0, 1, g::Msg<10, ()>>(),
            g::send::<2, 3, g::Msg<3, ()>>(),
        )
        .roll(),
        g::seq(
            g::send::<0, 1, g::Msg<20, ()>>(),
            g::send::<2, 3, g::Msg<3, ()>>(),
        ),
    );
    let _ = project::<2>(&protocol);
}
