//! Arbitrary user marker types must not satisfy the ControlMsg message bound.

use hibana::g;

struct MyControl;

fn main() {
    let _ = g::send::<1, 1, g::ControlMsg<40, MyControl>>();
}
