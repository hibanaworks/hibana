//! TopologyCommit is a wire control event and must cross roles.

use hibana::g;

fn main() {
    let _ = g::send::<1, 1, g::ControlMsg<22, g::control::TopologyCommit>>();
}
