use hibana::{
    g,
    g::advanced::CanonicalControl,
    substrate::{
        cap::advanced::ControlMint,
        cap::{ControlResourceKind, GenericCapToken, ResourceKind},
    },
};

use super::localside::{ControllerEndpoint, WorkerEndpoint, drive};

pub fn controller_send_u32<const LABEL: u8>(
    controller: &mut ControllerEndpoint<'_>,
    value: u32,
) {
    drive(
        controller
            .flow::<g::Msg<LABEL, u32>>()
            .expect("controller flow<u32>")
            .send(&value),
    )
    .expect("controller send<u32>");
}

pub fn controller_select<const LABEL: u8, K>(controller: &mut ControllerEndpoint<'_>)
where
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

pub fn worker_offer_decode_u32<const LABEL: u8>(worker: &mut WorkerEndpoint<'_>) -> u32 {
    let branch = drive(worker.offer()).expect("worker offer");
    assert_eq!(branch.label(), LABEL);
    drive(branch.decode::<g::Msg<LABEL, u32>>()).expect("worker decode<u32>")
}
