use hibana::{Endpoint, g, runtime::program::project};

fn main() {
    type Grant = g::Msg<1, Endpoint<'static, 10>>;
    let global = g::send::<0, 1, Grant>().roll();
    let _ = project::<0>(&global);
}
