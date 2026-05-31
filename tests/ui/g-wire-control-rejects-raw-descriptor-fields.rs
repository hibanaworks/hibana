use hibana::g;
use hibana::integration::{
    cap::{WireControlKind, GenericCapToken, WireControlEffect},
};

struct RawDescriptorFieldsKind;

impl WireControlKind for RawDescriptorFieldsKind {    const TAG: u8 = 0x71;
    const TAP_ID: u16 = 0x0471;
    const EFFECT: WireControlEffect = WireControlEffect::Fence;

    const SCOPE: u8 = 6;
    const PATH: u8 = 1;
    const SHOT: u8 = 1;
    const OP: u8 = 11;
}

fn main() {
    let _ = g::send::<
        0,
        1,
        g::Msg<121, GenericCapToken<RawDescriptorFieldsKind>>,
        0,
    >();
}
