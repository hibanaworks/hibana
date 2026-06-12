//! Control kinds are closed to Hibana-owned marker types.

use hibana::g;

struct MyControl;

impl g::control::ControlKind for MyControl {}

fn main() {
    let _ = MyControl;
}
