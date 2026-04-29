use hibana::{
    g,
    substrate::cap::{ControlResourceKind, GenericCapToken, ResourceKind},
};

use super::localside::{ControllerEndpoint, WorkerEndpoint, drive};

pub fn controller_send_u32<const LOGICAL_LABEL: u8>(
    controller: &mut ControllerEndpoint<'_>,
    value: u32,
) {
    drive(
        controller
            .flow::<g::Msg<LOGICAL_LABEL, u32>>()
            .expect("controller flow<u32>")
            .send(&value),
    )
    .expect("controller send<u32>");
}

pub fn controller_select<K, const LOGICAL_LABEL: u8>(controller: &mut ControllerEndpoint<'_>)
where
    K: ResourceKind + ControlResourceKind + 'static,
{
    drive(
        controller
            .flow::<g::Msg<LOGICAL_LABEL, GenericCapToken<K>, K>>()
            .expect("controller control flow")
            .send(()),
    )
    .expect("controller control send");
}

pub fn worker_offer_decode_u32<const LOGICAL_LABEL: u8>(worker: &mut WorkerEndpoint<'_>) -> u32 {
    let branch = drive(worker.offer()).expect("worker offer");
    assert_eq!(branch.label(), LOGICAL_LABEL);
    drive(branch.decode::<g::Msg<LOGICAL_LABEL, u32>>()).expect("worker decode<u32>")
}
