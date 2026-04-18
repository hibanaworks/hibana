use core::future::Future;

use hibana::{
    Endpoint, g,
    substrate::{
        SessionKit, Transport,
        cap::advanced::MintConfig,
        runtime::{Clock, LabelUniverse},
    },
};

pub type ControllerEndpoint<'a, T, U, C, const MAX_RV: usize> =
    Endpoint<'a, 0, SessionKit<'static, T, U, C, MAX_RV>, MintConfig>;
pub type WorkerEndpoint<'a, T, U, C, const MAX_RV: usize> =
    Endpoint<'a, 1, SessionKit<'static, T, U, C, MAX_RV>, MintConfig>;

pub(crate) fn drive<F: Future>(future: F) -> F::Output {
    super::drive(future)
}

pub fn controller_send_u8<const LABEL: u8, T, U, C, const MAX_RV: usize>(
    controller: &mut ControllerEndpoint<'_, T, U, C, MAX_RV>,
    value: u8,
) where
    T: Transport + 'static,
    U: LabelUniverse + 'static,
    C: Clock + 'static,
{
    let flow = controller
        .flow::<g::Msg<LABEL, u8>>()
        .expect("controller flow<u8>");
    drive(flow.send(&value)).expect("controller send<u8>");
}

pub fn worker_send_u8<const LABEL: u8, T, U, C, const MAX_RV: usize>(
    worker: &mut WorkerEndpoint<'_, T, U, C, MAX_RV>,
    value: u8,
) where
    T: Transport + 'static,
    U: LabelUniverse + 'static,
    C: Clock + 'static,
{
    let flow = worker.flow::<g::Msg<LABEL, u8>>().expect("worker flow<u8>");
    drive(flow.send(&value)).expect("worker send<u8>");
}

pub fn worker_recv_u8<const LABEL: u8, T, U, C, const MAX_RV: usize>(
    worker: &mut WorkerEndpoint<'_, T, U, C, MAX_RV>,
) -> u8
where
    T: Transport + 'static,
    U: LabelUniverse + 'static,
    C: Clock + 'static,
{
    drive(worker.recv::<g::Msg<LABEL, u8>>()).expect("worker recv<u8>")
}

pub fn controller_recv_u8<const LABEL: u8, T, U, C, const MAX_RV: usize>(
    controller: &mut ControllerEndpoint<'_, T, U, C, MAX_RV>,
) -> u8
where
    T: Transport + 'static,
    U: LabelUniverse + 'static,
    C: Clock + 'static,
{
    drive(controller.recv::<g::Msg<LABEL, u8>>()).expect("controller recv<u8>")
}

pub fn worker_offer_decode_u8<const LABEL: u8, T, U, C, const MAX_RV: usize>(
    worker: &mut WorkerEndpoint<'_, T, U, C, MAX_RV>,
) -> u8
where
    T: Transport + 'static,
    U: LabelUniverse + 'static,
    C: Clock + 'static,
{
    let branch = drive(worker.offer()).expect("worker offer");
    assert_eq!(branch.label(), LABEL);
    drive(branch.decode::<g::Msg<LABEL, u8>>()).expect("worker decode<u8>")
}
