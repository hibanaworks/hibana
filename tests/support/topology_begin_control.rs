use hibana::integration::cap::{WireControlEffect, WireControlKind};

pub(crate) const TOPOLOGY_BEGIN_CONTROL_LOGICAL: u8 = 121;
pub(crate) const TAG_TOPOLOGY_BEGIN_CONTROL: u8 = 0x71;
const TAP_TOPOLOGY_BEGIN_CONTROL: u16 = 0x0471;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TopologyBeginControl;

impl WireControlKind for TopologyBeginControl {
    const TAG: u8 = TAG_TOPOLOGY_BEGIN_CONTROL;
    const TAP_ID: u16 = TAP_TOPOLOGY_BEGIN_CONTROL;
    const EFFECT: WireControlEffect = WireControlEffect::TopologyBegin;
}
