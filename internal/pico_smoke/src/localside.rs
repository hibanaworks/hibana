use core::future::Future;

use hibana::{
    Endpoint, g,
    g::advanced::CanonicalControl,
    substrate::{
        SessionKit, Transport,
        cap::advanced::{ControlMint, MintConfig},
        cap::{ControlResourceKind, GenericCapToken, ResourceKind},
        runtime::{Clock, LabelUniverse},
    },
};

pub type ControllerEndpoint<'a, T, U, C, const MAX_RV: usize> =
    Endpoint<'a, 0, SessionKit<'static, T, U, C, MAX_RV>, MintConfig>;
pub type WorkerEndpoint<'a, T, U, C, const MAX_RV: usize> =
    Endpoint<'a, 1, SessionKit<'static, T, U, C, MAX_RV>, MintConfig>;

#[inline]
fn drive<F: Future>(future: F) -> F::Output {
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

pub fn controller_send_u32<const LABEL: u8, T, U, C, const MAX_RV: usize>(
    controller: &mut ControllerEndpoint<'_, T, U, C, MAX_RV>,
    value: u32,
) where
    T: Transport + 'static,
    U: LabelUniverse + 'static,
    C: Clock + 'static,
{
    let flow = controller
        .flow::<g::Msg<LABEL, u32>>()
        .expect("controller flow<u32>");
    drive(flow.send(&value)).expect("controller send<u32>");
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

pub fn controller_select<const LABEL: u8, K, T, U, C, const MAX_RV: usize>(
    controller: &mut ControllerEndpoint<'_, T, U, C, MAX_RV>,
) where
    T: Transport + 'static,
    U: LabelUniverse + 'static,
    C: Clock + 'static,
    K: ResourceKind + ControlResourceKind + ControlMint + 'static,
{
    let outcome = drive(
        controller
            .flow::<g::Msg<LABEL, GenericCapToken<K>, CanonicalControl<K>>>()
            .expect("controller control flow")
            .send(()),
    )
    .expect("controller control send");
    assert!(outcome.is_canonical());
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

pub fn worker_offer_decode_u32<const LABEL: u8, T, U, C, const MAX_RV: usize>(
    worker: &mut WorkerEndpoint<'_, T, U, C, MAX_RV>,
) -> u32
where
    T: Transport + 'static,
    U: LabelUniverse + 'static,
    C: Clock + 'static,
{
    let branch = drive(worker.offer()).expect("worker offer");
    assert_eq!(branch.label(), LABEL);
    drive(branch.decode::<g::Msg<LABEL, u32>>()).expect("worker decode<u32>")
}
