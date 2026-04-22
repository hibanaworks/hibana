use hibana::substrate::cap::{ControlResourceKind, advanced::LoopContinueKind};

fn main() {
    let _ = <LoopContinueKind as ControlResourceKind>::GRANTS;
}
