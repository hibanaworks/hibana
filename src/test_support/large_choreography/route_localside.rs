use hibana::g;

use super::localside::{ControllerEndpoint, WorkerEndpoint, drive};

#[inline(never)]
pub fn controller_send_u32<const LOGICAL_LABEL: u8>(
    controller: &mut ControllerEndpoint<'_>,
    value: u32,
) {
    crate::invariant_ok(drive(controller.send::<g::Msg<LOGICAL_LABEL, u32>>(&value)));
}

#[inline(never)]
pub fn worker_offer_decode_u32<const LOGICAL_LABEL: u8>(worker: &mut WorkerEndpoint<'_>) -> u32 {
    let branch = crate::invariant_ok(drive(worker.offer()));
    if branch.label() != LOGICAL_LABEL {
        crate::invariant();
    }
    crate::invariant_ok(drive(branch.recv::<g::Msg<LOGICAL_LABEL, u32>>()))
}
