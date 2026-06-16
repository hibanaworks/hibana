use hibana::g;

use super::localside::{ControllerEndpoint, WorkerEndpoint, drive};

#[inline(never)]
pub fn controller_send_u32<const LOGICAL_LABEL: u8>(
    controller: &mut ControllerEndpoint<'_>,
    value: u32,
) {
    drive(controller.send::<g::Msg<LOGICAL_LABEL, u32>>(&value)).expect("controller send<u32>");
}

#[inline(never)]
pub fn worker_offer_decode_u32<const LOGICAL_LABEL: u8>(worker: &mut WorkerEndpoint<'_>) -> u32 {
    let branch = drive(worker.offer()).expect("worker offer");
    assert_eq!(branch.label(), LOGICAL_LABEL);
    drive(branch.recv::<g::Msg<LOGICAL_LABEL, u32>>()).expect("worker recv<u32>")
}
