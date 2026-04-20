use core::future::Future;

use hibana::{Endpoint, g};

pub type ControllerEndpoint<'a> = Endpoint<'a, 0>;
pub type WorkerEndpoint<'a> = Endpoint<'a, 1>;

pub(crate) fn drive<F: Future>(future: F) -> F::Output {
    super::drive(future)
}

pub fn controller_send_u8<const LABEL: u8>(controller: &mut ControllerEndpoint<'_>, value: u8) {
    drive(
        controller
            .flow::<g::Msg<LABEL, u8>>()
            .expect("controller flow<u8>")
            .send(&value),
    )
    .expect("controller send<u8>");
}

pub fn worker_send_u8<const LABEL: u8>(worker: &mut WorkerEndpoint<'_>, value: u8) {
    drive(
        worker
            .flow::<g::Msg<LABEL, u8>>()
            .expect("worker flow<u8>")
            .send(&value),
    )
    .expect("worker send<u8>");
}

pub fn worker_recv_u8<const LABEL: u8>(worker: &mut WorkerEndpoint<'_>) -> u8 {
    drive(worker.recv::<g::Msg<LABEL, u8>>()).expect("worker recv<u8>")
}

pub fn controller_recv_u8<const LABEL: u8>(controller: &mut ControllerEndpoint<'_>) -> u8 {
    drive(controller.recv::<g::Msg<LABEL, u8>>()).expect("controller recv<u8>")
}

pub fn worker_offer_decode_u8<const LABEL: u8>(worker: &mut WorkerEndpoint<'_>) -> u8 {
    let branch = drive(worker.offer()).expect("worker offer");
    assert_eq!(branch.label(), LABEL);
    drive(branch.decode::<g::Msg<LABEL, u8>>()).expect("worker decode<u8>")
}
