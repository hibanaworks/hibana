use hibana::{Endpoint, g, runtime::program::project};

fn main() {
    type Grant = g::Msg<1, Endpoint<'static, 10>>;
    let global = g::par(
        g::send::<0, 1, Grant>(),
        g::send::<10, 3, g::Msg<2, u32>>(),
    );
    let _ = project::<0>(&global);
}
