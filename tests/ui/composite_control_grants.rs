use hibana::integration::cap::{ControlResourceKind, advanced::LoopContinueKind};

fn main() {
    let _ = <LoopContinueKind as ControlResourceKind>::GRANTS;
}
