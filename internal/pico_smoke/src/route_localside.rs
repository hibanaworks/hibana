use hibana::{
    g,
    g::advanced::CanonicalControl,
    substrate::{
        Transport,
        cap::advanced::ControlMint,
        cap::{ControlResourceKind, GenericCapToken, ResourceKind},
        runtime::{Clock, LabelUniverse},
    },
};

use super::localside::{ControllerEndpoint, WorkerEndpoint, drive};

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
