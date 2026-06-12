//! TopologyBegin is a wire control event and must cross roles.

use hibana::g;

fn main() {
    let _ = g::send::<1, 1, g::ControlMsg<20, g::control::TopologyBegin>>();
}
