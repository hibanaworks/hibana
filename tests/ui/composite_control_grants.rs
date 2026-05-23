use hibana::integration::cap::{ControlResourceKind, control::LoopContinueKind};

fn main() {
    let _ = <LoopContinueKind as ControlResourceKind>::GRANTS;
}
