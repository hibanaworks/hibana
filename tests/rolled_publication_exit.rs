//! A completed rolled publication must not select a previous descendant arm.
use core::{
    cell::Cell,
    future::Future,
    pin::pin,
    task::{Context, Poll},
};
use hibana::{
    g::{self, Message},
    runtime::{
        SessionKitStorage,
        ids::SessionId,
        program::{RoleProgram, project},
        resolver::{DecisionArm, ResolverError, ResolverRef},
    },
};
use std::{
    sync::{Arc, Mutex},
    task::{Wake, Waker},
};
mod common;
mod p {
    use hibana::g;
    pub const TX_WIRE: u8 = 7;
    pub const UDP: u8 = 4;
    pub const ADAPTER_RESULT: u16 = 1001;
    pub trait Publication {
        type Datagram: g::Message<Payload = u64>;
        type Accepted: g::Message<Payload = u64>;
        type Rejected: g::Message<Payload = u64>;
        type Settled: g::Message<Payload = u64>;
    }
    pub struct Emission<const D: u8, const A: u8, const R: u8, const S: u8>;
    impl<const D: u8, const A: u8, const R: u8, const S: u8> Publication for Emission<D, A, R, S> {
        type Datagram = g::Msg<D, u64>;
        type Accepted = g::Msg<A, u64>;
        type Rejected = g::Msg<R, u64>;
        type Settled = g::Msg<S, u64>;
    }
    pub trait TransmitPhase {
        type Data: Publication;
        type Ack: Publication;
        type Probe: Publication;
        type WireBoundary: g::Message<Payload = u64>;
        type WireBoundarySeen: g::Message<Payload = u64>;
    }
    pub struct InitialTransmit;
    impl TransmitPhase for InitialTransmit {
        type Data = Emission<45, 46, 47, 48>;
        type Ack = Emission<37, 38, 39, 40>;
        type Probe = Emission<41, 42, 43, 44>;
        type WireBoundary = g::Msg<59, u64>;
        type WireBoundarySeen = g::Msg<60, u64>;
    }
}
async fn yield_now() {
    let mut yielded = false;
    core::future::poll_fn(|cx| {
        if yielded {
            Poll::Ready(())
        } else {
            yielded = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    })
    .await;
}

struct WakeCount(Mutex<usize>);
impl Wake for WakeCount {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        *self.0.lock().unwrap() += 1;
    }
}

#[test]
fn rolled_publication_exit_keeps_only_live_descendant_selection() {
    {
        for publications in [&[0, 1, 2][..], &[0][..], &[1][..], &[2][..], &[][..]] {
            run_publications(publications);
        }
    }
}

type Publication<P> = g::Seq<
    g::Send<{ p::TX_WIRE }, { p::UDP }, <P as p::Publication>::Datagram>,
    g::Seq<
        g::Resolve<
            g::Route<
                g::Send<{ p::UDP }, { p::TX_WIRE }, <P as p::Publication>::Accepted>,
                g::Send<{ p::UDP }, { p::TX_WIRE }, <P as p::Publication>::Rejected>,
            >,
            { p::ADAPTER_RESULT },
        >,
        g::Send<{ p::TX_WIRE }, { p::UDP }, <P as p::Publication>::Settled>,
    >,
>;
fn publication<P: p::Publication>() -> g::Program<Publication<P>> {
    g::seq(
        g::send::<{ p::TX_WIRE }, { p::UDP }, P::Datagram>(),
        g::seq(
            g::route(
                g::send::<{ p::UDP }, { p::TX_WIRE }, P::Accepted>(),
                g::send::<{ p::UDP }, { p::TX_WIRE }, P::Rejected>(),
            )
            .resolve::<{ p::ADAPTER_RESULT }>(),
            g::send::<{ p::TX_WIRE }, { p::UDP }, P::Settled>(),
        ),
    )
}
fn minimal_programs() -> (RoleProgram<{ p::TX_WIRE }>, RoleProgram<{ p::UDP }>) {
    type Stage = p::InitialTransmit;
    let global =
        g::route(
            publication::<<Stage as p::TransmitPhase>::Data>(),
            g::route(
                publication::<<Stage as p::TransmitPhase>::Ack>(),
                g::route(
                    publication::<<Stage as p::TransmitPhase>::Probe>(),
                    g::seq(
                        g::send::<
                            { p::TX_WIRE },
                            { p::UDP },
                            <Stage as p::TransmitPhase>::WireBoundary,
                        >(),
                        g::send::<
                            { p::UDP },
                            { p::TX_WIRE },
                            <Stage as p::TransmitPhase>::WireBoundarySeen,
                        >(),
                    ),
                ),
            ),
        )
        .roll();
    (project(&global), project(&global))
}

fn run_publications(publications: &[u8]) {
    let (sender_program, receiver_program) = minimal_programs();
    let result = Cell::new(None);
    let queues = common::TestTransport::new();
    let mut slab = vec![0; 256 * 1024];
    let mut storage = Box::new(SessionKitStorage::uninit());
    let kit = storage.init();
    let session = SessionId::new(171);
    let rendezvous = kit.rendezvous(&mut slab, queues.clone()).unwrap();
    rendezvous
        .set_resolver(
            &receiver_program,
            ResolverRef::<{ p::ADAPTER_RESULT }>::decision_state(&result, |state| {
                state.get().ok_or_else(ResolverError::reject)
            }),
        )
        .unwrap();
    let mut sender = rendezvous.enter(session, &sender_program).unwrap();
    let mut receiver = rendezvous.enter(session, &receiver_program).unwrap();
    type Data = <p::InitialTransmit as p::TransmitPhase>::Data;
    type Ack = <p::InitialTransmit as p::TransmitPhase>::Ack;
    type Probe = <p::InitialTransmit as p::TransmitPhase>::Probe;
    type Boundary = <p::InitialTransmit as p::TransmitPhase>::WireBoundary;
    type Seen = <p::InitialTransmit as p::TransmitPhase>::WireBoundarySeen;
    let parked = Cell::new(0);
    let send = async {
        macro_rules! publish {
            ($pub:ty, $id:expr) => {{
                sender
                    .send::<<$pub as p::Publication>::Datagram>(&$id)
                    .await?;
                assert_eq!(
                    sender.recv::<<$pub as p::Publication>::Accepted>().await?,
                    $id
                );
                sender
                    .send::<<$pub as p::Publication>::Settled>(&$id)
                    .await?;
                // The receiver must actually park before the next route choice.
                while parked.get() <= $id {
                    yield_now().await;
                }
            }};
        }
        for (id, kind) in publications.iter().enumerate() {
            let id = id as u64;
            match kind {
                0 => publish!(Data, id),
                1 => publish!(Ack, id),
                2 => publish!(Probe, id),
                _ => unreachable!(),
            }
        }
        let end = publications.len() as u64;
        sender.send::<Boundary>(&end).await?;
        assert_eq!(sender.recv::<Seen>().await?, end);
        Ok::<_, hibana::EndpointError>(())
    };
    let receive = async {
        macro_rules! accept {
            ($pub:ty, $id:expr) => {{
                {
                    let mut offer = pin!(receiver.offer());
                    let branch = core::future::poll_fn(|cx| match offer.as_mut().poll(cx) {
                        Poll::Pending => {
                            parked.set($id);
                            Poll::Pending
                        }
                        Poll::Ready(value) => Poll::Ready(value),
                    })
                    .await?;
                    assert_eq!(
                        branch.label(),
                        <$pub as p::Publication>::Datagram::LOGICAL_LABEL
                    );
                    assert_eq!(
                        branch.recv::<<$pub as p::Publication>::Datagram>().await?,
                        $id
                    );
                }
                result.set(Some(DecisionArm::Left));
                receiver
                    .send::<<$pub as p::Publication>::Accepted>(&$id)
                    .await?;
                assert_eq!(
                    receiver.recv::<<$pub as p::Publication>::Settled>().await?,
                    $id
                );
                result.set(None);
            }};
        }
        for (id, kind) in publications.iter().enumerate() {
            let id = id as u64;
            match kind {
                0 => accept!(Data, id),
                1 => accept!(Ack, id),
                2 => accept!(Probe, id),
                _ => unreachable!(),
            }
        }
        let end = publications.len() as u64;
        {
            let mut offer = pin!(receiver.offer());
            let branch = core::future::poll_fn(|cx| match offer.as_mut().poll(cx) {
                Poll::Pending => {
                    parked.set(end);
                    Poll::Pending
                }
                Poll::Ready(value) => Poll::Ready(value),
            })
            .await?;
            assert_eq!(branch.label(), Boundary::LOGICAL_LABEL);
            assert_eq!(branch.recv::<Boundary>().await?, end);
        }
        receiver.send::<Seen>(&end).await?;
        Ok::<_, hibana::EndpointError>(())
    };
    let wake = Arc::new(WakeCount(Mutex::new(0)));
    let waker: Waker = wake.clone().into();
    let mut cx = Context::from_waker(&waker);
    let mut joined = Box::pin(futures::future::try_join(send, receive));
    for _ in 0..128 {
        *wake.0.lock().unwrap() = 0;
        match joined.as_mut().poll(&mut cx) {
            Poll::Ready(value) => {
                value.unwrap_or_else(|error| panic!(
                    "Initial publication must retain ownership: arms={publications:?}, {error:?}"));
                assert!(queues.queue_is_empty());
                return;
            }
            Poll::Pending => assert!(
                *wake.0.lock().unwrap() > 0,
                "parked publication must have a real wake"
            ),
        }
    }
    panic!("publication route did not complete within the bounded poll schedule");
}
