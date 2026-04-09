use hibana::substrate::cap::ControlResourceKind;
use hibana::substrate::cap::ResourceKind;
use hibana::substrate::cap::advanced::ControlMint;

pub trait ScenarioHarness {
    type ControllerEndpoint<'a>
    where
        Self: 'a;
    type WorkerEndpoint<'a>
    where
        Self: 'a;

    fn controller_send_u8<const LABEL: u8>(
        controller: &mut Self::ControllerEndpoint<'_>,
        value: u8,
    );

    fn controller_send_u32<const LABEL: u8>(
        controller: &mut Self::ControllerEndpoint<'_>,
        value: u32,
    );

    fn worker_send_u8<const LABEL: u8>(worker: &mut Self::WorkerEndpoint<'_>, value: u8);

    fn worker_recv_u8<const LABEL: u8>(worker: &mut Self::WorkerEndpoint<'_>) -> u8;

    fn controller_recv_u8<const LABEL: u8>(controller: &mut Self::ControllerEndpoint<'_>) -> u8;

    fn controller_select<'a, const LABEL: u8, K>(controller: &mut Self::ControllerEndpoint<'a>)
    where
        K: ResourceKind + ControlResourceKind + ControlMint + 'a + 'static;

    fn worker_offer_decode_u32<const LABEL: u8>(worker: &mut Self::WorkerEndpoint<'_>) -> u32;
}

#[cfg(test)]
pub struct FixtureHarness;

#[cfg(test)]
const _: FixtureHarness = FixtureHarness;

#[cfg(test)]
impl ScenarioHarness for FixtureHarness {
    type ControllerEndpoint<'a> = ();
    type WorkerEndpoint<'a> = ();

    fn controller_send_u8<const LABEL: u8>(
        _controller: &mut Self::ControllerEndpoint<'_>,
        _value: u8,
    ) {
    }

    fn controller_send_u32<const LABEL: u8>(
        _controller: &mut Self::ControllerEndpoint<'_>,
        _value: u32,
    ) {
    }

    fn worker_send_u8<const LABEL: u8>(_worker: &mut Self::WorkerEndpoint<'_>, _value: u8) {}

    fn worker_recv_u8<const LABEL: u8>(_worker: &mut Self::WorkerEndpoint<'_>) -> u8 {
        0
    }

    fn controller_recv_u8<const LABEL: u8>(_controller: &mut Self::ControllerEndpoint<'_>) -> u8 {
        0
    }

    fn controller_select<'a, const LABEL: u8, K>(_controller: &mut Self::ControllerEndpoint<'a>)
    where
        K: ResourceKind + ControlResourceKind + ControlMint + 'a + 'static,
    {
    }

    fn worker_offer_decode_u32<const LABEL: u8>(_worker: &mut Self::WorkerEndpoint<'_>) -> u32 {
        0
    }
}
