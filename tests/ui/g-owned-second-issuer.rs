use hibana::{Endpoint, g, runtime::program::project};

fn main() {
    type Grant = g::Msg<1, Endpoint<'static, 10>>;
    let global = g::seq(g::send::<0, 1, Grant>(), g::send::<2, 1, Grant>());
    let _ = project::<0>(&global);
}
