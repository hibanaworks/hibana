use core::future::Future;

use hibana::{Endpoint, g};

pub type ControllerEndpoint<'a> = Endpoint<'a, 0>;
pub type WorkerEndpoint<'a> = Endpoint<'a, 1>;

#[inline(always)]
pub(crate) fn drive<F: Future>(future: F) -> F::Output {
    super::drive(future)
}

#[inline(never)]
pub fn controller_send_u8<const LOGICAL_LABEL: u8>(
    controller: &mut ControllerEndpoint<'_>,
    value: u8,
) {
    crate::invariant_ok(drive(controller.send::<g::Msg<LOGICAL_LABEL, u8>>(&value)));
}

#[inline(never)]
pub fn worker_send_u8<const LOGICAL_LABEL: u8>(worker: &mut WorkerEndpoint<'_>, value: u8) {
    crate::invariant_ok(drive(worker.send::<g::Msg<LOGICAL_LABEL, u8>>(&value)));
}

#[inline(never)]
pub fn worker_recv_u8<const LOGICAL_LABEL: u8>(worker: &mut WorkerEndpoint<'_>) -> u8 {
    crate::invariant_ok(drive(worker.recv::<g::Msg<LOGICAL_LABEL, u8>>()))
}

#[inline(never)]
pub fn controller_recv_u8<const LOGICAL_LABEL: u8>(controller: &mut ControllerEndpoint<'_>) -> u8 {
    crate::invariant_ok(drive(controller.recv::<g::Msg<LOGICAL_LABEL, u8>>()))
}

#[inline(never)]
pub fn worker_offer_recv_u8<const LOGICAL_LABEL: u8>(worker: &mut WorkerEndpoint<'_>) -> u8 {
    let branch = crate::invariant_ok(drive(worker.offer()));
    if branch.label() != LOGICAL_LABEL {
        crate::invariant();
    }
    crate::invariant_ok(drive(branch.recv::<g::Msg<LOGICAL_LABEL, u8>>()))
}
