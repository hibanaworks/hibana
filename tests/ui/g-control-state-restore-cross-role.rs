//! StateRestore is a local control event and must not cross roles.

use hibana::g;

fn main() {
    let _ = g::send::<1, 2, g::ControlMsg<31, g::control::StateRestore>>();
}
