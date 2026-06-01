use hibana::integration::cap::{WireControlKind, control::LoopContinueKind};

fn main() {
    let _ = <LoopContinueKind as WireControlKind>::GRANTS;
}
