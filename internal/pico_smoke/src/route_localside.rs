use hibana::{
    g,
    substrate::cap::{ControlResourceKind, GenericCapToken, ResourceKind},
};

use super::{
    localside::{ControllerEndpoint, WorkerEndpoint, drive},
    route_control_kinds::RouteControl,
};

pub fn controller_send_u32<const LABEL: u8>(controller: &mut ControllerEndpoint<'_>, value: u32) {
    drive(
        controller
            .flow::<g::Msg<LABEL, u32>>()
            .expect("controller flow<u32>")
            .send(&value),
    )
    .expect("controller send<u32>");
}

pub(crate) trait PicoSelectableControl:
    ResourceKind + ControlResourceKind + 'static
{
    fn select(controller: &mut ControllerEndpoint<'_>);
}

impl<const ARM: u8> PicoSelectableControl for RouteControl<112, ARM> {
    fn select(controller: &mut ControllerEndpoint<'_>) {
        let outcome = drive(
            controller
                .flow::<g::Msg<112, GenericCapToken<Self>, Self>>()
                .expect("controller control flow")
                .send(()),
        )
        .expect("controller control send");
        assert!(outcome.is_canonical());
    }
}

impl<const ARM: u8> PicoSelectableControl for RouteControl<113, ARM> {
    fn select(controller: &mut ControllerEndpoint<'_>) {
        let outcome = drive(
            controller
                .flow::<g::Msg<113, GenericCapToken<Self>, Self>>()
                .expect("controller control flow")
                .send(()),
        )
        .expect("controller control send");
        assert!(outcome.is_canonical());
    }
}

impl<const ARM: u8> PicoSelectableControl for RouteControl<114, ARM> {
    fn select(controller: &mut ControllerEndpoint<'_>) {
        let outcome = drive(
            controller
                .flow::<g::Msg<114, GenericCapToken<Self>, Self>>()
                .expect("controller control flow")
                .send(()),
        )
        .expect("controller control send");
        assert!(outcome.is_canonical());
    }
}

impl<const ARM: u8> PicoSelectableControl for RouteControl<115, ARM> {
    fn select(controller: &mut ControllerEndpoint<'_>) {
        let outcome = drive(
            controller
                .flow::<g::Msg<115, GenericCapToken<Self>, Self>>()
                .expect("controller control flow")
                .send(()),
        )
        .expect("controller control send");
        assert!(outcome.is_canonical());
    }
}

impl<const ARM: u8> PicoSelectableControl for RouteControl<116, ARM> {
    fn select(controller: &mut ControllerEndpoint<'_>) {
        let outcome = drive(
            controller
                .flow::<g::Msg<116, GenericCapToken<Self>, Self>>()
                .expect("controller control flow")
                .send(()),
        )
        .expect("controller control send");
        assert!(outcome.is_canonical());
    }
}

impl<const ARM: u8> PicoSelectableControl for RouteControl<117, ARM> {
    fn select(controller: &mut ControllerEndpoint<'_>) {
        let outcome = drive(
            controller
                .flow::<g::Msg<117, GenericCapToken<Self>, Self>>()
                .expect("controller control flow")
                .send(()),
        )
        .expect("controller control send");
        assert!(outcome.is_canonical());
    }
}

impl<const ARM: u8> PicoSelectableControl for RouteControl<118, ARM> {
    fn select(controller: &mut ControllerEndpoint<'_>) {
        let outcome = drive(
            controller
                .flow::<g::Msg<118, GenericCapToken<Self>, Self>>()
                .expect("controller control flow")
                .send(()),
        )
        .expect("controller control send");
        assert!(outcome.is_canonical());
    }
}

impl<const ARM: u8> PicoSelectableControl for RouteControl<119, ARM> {
    fn select(controller: &mut ControllerEndpoint<'_>) {
        let outcome = drive(
            controller
                .flow::<g::Msg<119, GenericCapToken<Self>, Self>>()
                .expect("controller control flow")
                .send(()),
        )
        .expect("controller control send");
        assert!(outcome.is_canonical());
    }
}

impl<const ARM: u8> PicoSelectableControl for RouteControl<120, ARM> {
    fn select(controller: &mut ControllerEndpoint<'_>) {
        let outcome = drive(
            controller
                .flow::<g::Msg<120, GenericCapToken<Self>, Self>>()
                .expect("controller control flow")
                .send(()),
        )
        .expect("controller control send");
        assert!(outcome.is_canonical());
    }
}

impl<const ARM: u8> PicoSelectableControl for RouteControl<121, ARM> {
    fn select(controller: &mut ControllerEndpoint<'_>) {
        let outcome = drive(
            controller
                .flow::<g::Msg<121, GenericCapToken<Self>, Self>>()
                .expect("controller control flow")
                .send(()),
        )
        .expect("controller control send");
        assert!(outcome.is_canonical());
    }
}

impl<const ARM: u8> PicoSelectableControl for RouteControl<122, ARM> {
    fn select(controller: &mut ControllerEndpoint<'_>) {
        let outcome = drive(
            controller
                .flow::<g::Msg<122, GenericCapToken<Self>, Self>>()
                .expect("controller control flow")
                .send(()),
        )
        .expect("controller control send");
        assert!(outcome.is_canonical());
    }
}

impl<const ARM: u8> PicoSelectableControl for RouteControl<123, ARM> {
    fn select(controller: &mut ControllerEndpoint<'_>) {
        let outcome = drive(
            controller
                .flow::<g::Msg<123, GenericCapToken<Self>, Self>>()
                .expect("controller control flow")
                .send(()),
        )
        .expect("controller control send");
        assert!(outcome.is_canonical());
    }
}

impl<const ARM: u8> PicoSelectableControl for RouteControl<124, ARM> {
    fn select(controller: &mut ControllerEndpoint<'_>) {
        let outcome = drive(
            controller
                .flow::<g::Msg<124, GenericCapToken<Self>, Self>>()
                .expect("controller control flow")
                .send(()),
        )
        .expect("controller control send");
        assert!(outcome.is_canonical());
    }
}

impl<const ARM: u8> PicoSelectableControl for RouteControl<125, ARM> {
    fn select(controller: &mut ControllerEndpoint<'_>) {
        let outcome = drive(
            controller
                .flow::<g::Msg<125, GenericCapToken<Self>, Self>>()
                .expect("controller control flow")
                .send(()),
        )
        .expect("controller control send");
        assert!(outcome.is_canonical());
    }
}

impl<const ARM: u8> PicoSelectableControl for RouteControl<126, ARM> {
    fn select(controller: &mut ControllerEndpoint<'_>) {
        let outcome = drive(
            controller
                .flow::<g::Msg<126, GenericCapToken<Self>, Self>>()
                .expect("controller control flow")
                .send(()),
        )
        .expect("controller control send");
        assert!(outcome.is_canonical());
    }
}

impl<const ARM: u8> PicoSelectableControl for RouteControl<127, ARM> {
    fn select(controller: &mut ControllerEndpoint<'_>) {
        let outcome = drive(
            controller
                .flow::<g::Msg<127, GenericCapToken<Self>, Self>>()
                .expect("controller control flow")
                .send(()),
        )
        .expect("controller control send");
        assert!(outcome.is_canonical());
    }
}

pub fn controller_select<K>(controller: &mut ControllerEndpoint<'_>)
where
    K: PicoSelectableControl,
{
    K::select(controller);
}

pub fn worker_offer_decode_u32<const LABEL: u8>(worker: &mut WorkerEndpoint<'_>) -> u32 {
    let branch = drive(worker.offer()).expect("worker offer");
    assert_eq!(branch.label(), LABEL);
    drive(branch.decode::<g::Msg<LABEL, u32>>()).expect("worker decode<u32>")
}
