//! Local control events must not cross roles.

use hibana::g;

fn main() {
    let _ = g::send::<1, 2, g::ControlMsg<1, g::control::LoopContinue>>();
}
