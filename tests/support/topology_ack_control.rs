use hibana::integration::cap::{WireControlEffect, WireControlKind};

pub(crate) const TOPOLOGY_ACK_CONTROL_LOGICAL: u8 = 122;
pub(crate) const TAG_TOPOLOGY_ACK_CONTROL: u8 = 0x72;
const TAP_TOPOLOGY_ACK_CONTROL: u16 = 0x0472;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TopologyAckControl;

impl WireControlKind for TopologyAckControl {
    const TAG: u8 = TAG_TOPOLOGY_ACK_CONTROL;
    const TAP_ID: u16 = TAP_TOPOLOGY_ACK_CONTROL;
    const EFFECT: WireControlEffect = WireControlEffect::TopologyAck;
}
