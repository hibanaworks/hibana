use core::task::{Context, Poll};

use hibana::{
    g::{self, Msg},
    runtime::{
        SessionKitStorage,
        program::{RoleProgram, project},
        resolver::{DecisionArm, ResolverError, ResolverRef},
        transport::{Outgoing, PortOpen, ReceivedFrame, Transport, TransportError},
    },
};

const ROUTE_RESOLVER: u16 = 41;
static UNIT: () = ();

struct NoopTransport;
struct NoopTx;
struct NoopRx;

impl Transport for NoopTransport {
    type Tx<'a> = NoopTx;
    type Rx<'a> = NoopRx;

    fn open<'a>(&'a self, _: PortOpen) -> (Self::Tx<'a>, Self::Rx<'a>) {
        (NoopTx, NoopRx)
    }

    fn poll_send<'a, 'f>(
        &self,
        _: &'a mut Self::Tx<'a>,
        _: Outgoing<'f>,
        _: &mut Context<'_>,
    ) -> Poll<Result<(), TransportError>>
    where
        'a: 'f,
    {
        Poll::Pending
    }

    fn cancel_send<'a>(&self, _: &'a mut Self::Tx<'a>) {}

    fn poll_recv<'a>(
        &'a self,
        _: &'a mut Self::Rx<'a>,
        _: &mut Context<'_>,
    ) -> Poll<Result<ReceivedFrame<'a>, TransportError>> {
        Poll::Pending
    }

    fn requeue<'a>(&self, _: &mut Self::Rx<'a>) -> Result<(), TransportError> {
        Ok(())
    }
}

struct ResolverOwner {
    inner: ResolverRef<'static, ROUTE_RESOLVER>,
}

fn choose_left(_: &()) -> Result<DecisionArm, ResolverError> {
    Ok(DecisionArm::Left)
}

fn choose_through_owner(owner: &ResolverOwner) -> Result<DecisionArm, ResolverError> {
    owner.inner.decide()
}

fn program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let left = g::par(
        g::send::<0, 1, Msg<11, u8>>(),
        g::send::<0, 2, Msg<12, u8>>(),
    );
    let right = g::send::<0, 1, Msg<13, u8>>();
    project(&g::route(left, right).resolve::<ROUTE_RESOLVER>())
}

fn main() {
    let role0 = program::<0>();
    let owner = ResolverOwner {
        inner: ResolverRef::<ROUTE_RESOLVER>::decision_state(&UNIT, choose_left),
    };
    let resolver = ResolverRef::<ROUTE_RESOLVER>::decision_state(&owner, choose_through_owner);

    let mut slab = [0u8; 8192];
    let mut storage = SessionKitStorage::<NoopTransport>::uninit();
    let kit = storage.init();
    let rv = kit.rendezvous(&mut slab, NoopTransport).unwrap();
    rv.set_resolver(&role0, resolver).unwrap();
}
