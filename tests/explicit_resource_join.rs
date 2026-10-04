//! No new Hibana API: explicit operation completion and private resource ownership.
mod common;
use common::TestTransport;
use core::{
    cell::Cell,
    future::Future,
    pin::pin,
    task::{Context, Poll, Waker},
};
use futures::{channel::oneshot, executor::block_on, join};
use hibana::{
    Endpoint, EndpointError,
    g::{self, Msg},
    runtime::{
        SessionKitStorage,
        ids::SessionId,
        program::{RoleProgram, project},
    },
};

const RX: u8 = 0;
const TX: u8 = 1;
const OWNER: u8 = 2;
const NEXT: u8 = 3;
type WaitRx = Msg<0, u64>;
type RxDone = Msg<1, u64>;
type RxFailed = Msg<2, u64>;
type WaitTx = Msg<3, u64>;
type TxDone = Msg<4, u64>;
type TxFailed = Msg<5, u64>;
type Released = Msg<6, u64>;
type Aborted = Msg<7, u64>;
type Taken = Msg<8, u64>;
type RxFlow = g::Seq<
    g::Send<OWNER, RX, WaitRx>,
    g::Route<g::Send<RX, OWNER, RxDone>, g::Send<RX, OWNER, RxFailed>>,
>;
type TxFlow = g::Seq<
    g::Send<OWNER, TX, WaitTx>,
    g::Route<g::Send<TX, OWNER, TxDone>, g::Send<TX, OWNER, TxFailed>>,
>;
type Flow = g::Seq<
    g::Par<RxFlow, TxFlow>,
    g::Seq<
        g::Route<g::Send<OWNER, NEXT, Released>, g::Send<OWNER, NEXT, Aborted>>,
        g::Send<NEXT, OWNER, Taken>,
    >,
>;
fn choreography() -> g::Program<Flow> {
    g::seq(
        g::par(
            g::seq(
                g::send::<OWNER, RX, WaitRx>(),
                g::route(
                    g::send::<RX, OWNER, RxDone>(),
                    g::send::<RX, OWNER, RxFailed>(),
                ),
            ),
            g::seq(
                g::send::<OWNER, TX, WaitTx>(),
                g::route(
                    g::send::<TX, OWNER, TxDone>(),
                    g::send::<TX, OWNER, TxFailed>(),
                ),
            ),
        ),
        g::seq(
            g::route(
                g::send::<OWNER, NEXT, Released>(),
                g::send::<OWNER, NEXT, Aborted>(),
            ),
            g::send::<NEXT, OWNER, Taken>(),
        ),
    )
}

mod resource {
    use super::*;
    pub struct Operation {
        pub serial: u64,
    }
    pub struct Resource<'a> {
        operation: &'a Operation,
        uses: Cell<usize>,
    }
    pub struct Use<'a> {
        uses: &'a Cell<usize>,
    }
    pub struct Joined<'a> {
        operation: &'a Operation,
    }
    pub struct Owner<'a> {
        resource: Resource<'a>,
    }
    impl<'a> Owner<'a> {
        pub fn new(operation: &'a Operation) -> Self {
            Self {
                resource: Resource {
                    operation,
                    uses: Cell::new(0),
                },
            }
        }
        pub fn usage(&self) -> Use<'_> {
            Use {
                uses: &self.resource.uses,
            }
        }
        pub fn return_owned(self, joined: Joined<'_>) -> Result<Resource<'a>, Self> {
            if core::ptr::eq(self.resource.operation, joined.operation) {
                Ok(self.resource)
            } else {
                Err(self)
            }
        }
    }
    impl Use<'_> {
        pub fn work(&self) {
            self.uses.set(self.uses.get() + 1);
        }
    }
    impl Resource<'_> {
        pub fn retire(self) -> usize {
            self.uses.get()
        }
    }
    // The only normal-completion constructor lives beside actual recv operations.
    pub async fn complete<'a>(
        endpoint: &mut Endpoint<'_, OWNER>,
        operation: &'a Operation,
    ) -> Result<Option<Joined<'a>>, EndpointError> {
        endpoint.send::<WaitRx>(&operation.serial).await?;
        endpoint.send::<WaitTx>(&operation.serial).await?;
        // Parallel branches may arrive in either order. Consume the actual
        // offered label rather than assuming RX precedes TX.
        let first = endpoint.offer().await?;
        let (first_is_rx, first_done) = match first.label() {
            1 => (true, first.recv::<RxDone>().await? == operation.serial),
            2 => {
                let _ = first.recv::<RxFailed>().await?;
                (true, false)
            }
            4 => (false, first.recv::<TxDone>().await? == operation.serial),
            5 => {
                let _ = first.recv::<TxFailed>().await?;
                (false, false)
            }
            other => panic!("unexpected completion label {other}"),
        };
        let second = endpoint.offer().await?;
        let second_done = match (first_is_rx, second.label()) {
            (true, 4) => second.recv::<TxDone>().await? == operation.serial,
            (true, 5) => {
                let _ = second.recv::<TxFailed>().await?;
                false
            }
            (false, 1) => second.recv::<RxDone>().await? == operation.serial,
            (false, 2) => {
                let _ = second.recv::<RxFailed>().await?;
                false
            }
            (_, other) => panic!("duplicate or unexpected completion label {other}"),
        };
        let (rx_done, tx_done) = if first_is_rx {
            (first_done, second_done)
        } else {
            (second_done, first_done)
        };
        // These are results of the two actual current-operation receives, not
        // an independently updated lifecycle or a transport acceptance flag.
        if rx_done && tx_done {
            endpoint.send::<Released>(&operation.serial).await?;
        } else {
            endpoint.send::<Aborted>(&operation.serial).await?;
        }
        let taken = endpoint.recv::<Taken>().await?;
        Ok((rx_done && tx_done && taken == operation.serial).then_some(Joined { operation }))
    }
}

fn once<F: Future>(future: core::pin::Pin<&mut F>) -> Poll<F::Output> {
    future.poll(&mut Context::from_waker(Waker::noop()))
}
fn scenario(rx_ok: bool, tx_ok: bool, stale: bool) {
    let global = choreography();
    let rp: RoleProgram<RX> = project(&global);
    let tp: RoleProgram<TX> = project(&global);
    let op: RoleProgram<OWNER> = project(&global);
    let np: RoleProgram<NEXT> = project(&global);
    let mut slab = [0; 16384];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, TestTransport::new())
        .unwrap();
    let id = SessionId::new(0x7070);
    let mut rx = rv.enter(id, &rp).unwrap();
    let mut tx = rv.enter(id, &tp).unwrap();
    let mut owner_ep = rv.enter(id, &op).unwrap();
    let mut next = rv.enter(id, &np).unwrap();
    let operation = resource::Operation { serial: 7 };
    let owner = resource::Owner::new(&operation);
    let joined;
    {
        let rx_use = owner.usage();
        let tx_use = owner.usage();
        let (release_io, io_finished) = oneshot::channel::<()>();
        let mut completion = pin!(resource::complete(&mut owner_ep, &operation));
        assert!(once(completion.as_mut()).is_pending());
        block_on(async {
            assert_eq!(rx.recv::<WaitRx>().await.unwrap(), 7);
            rx_use.work();
            if rx_ok {
                rx.send::<RxDone>(&if stale { 6 } else { 7 }).await.unwrap();
            } else {
                rx.send::<RxFailed>(&7).await.unwrap();
            }
        });
        assert!(
            once(completion.as_mut()).is_pending(),
            "RX completion does not imply TX completion"
        );
        let mut tx_task = pin!(async {
            assert_eq!(tx.recv::<WaitTx>().await?, 7);
            // Receiving the request/acceptance is not actual IO completion.
            io_finished.await.unwrap();
            tx_use.work();
            if tx_ok {
                tx.send::<TxDone>(&7).await?;
            } else {
                tx.send::<TxFailed>(&7).await?;
            }
            Ok::<(), EndpointError>(())
        });
        assert!(once(tx_task.as_mut()).is_pending());
        assert!(once(completion.as_mut()).is_pending());
        release_io.send(()).unwrap();
        joined = block_on(async {
            let consumer = async {
                let branch = next.offer().await?;
                let success = branch.label() == 6;
                let value = if success {
                    branch.recv::<Released>().await?
                } else {
                    branch.recv::<Aborted>().await?
                };
                assert_eq!(success, rx_ok && tx_ok && !stale);
                next.send::<Taken>(&value).await?;
                Ok::<(), EndpointError>(())
            };
            let (done, tx, received) = join!(completion.as_mut(), tx_task.as_mut(), consumer);
            tx.expect("TX");
            let done = done.expect("OWNER");
            received.expect("NEXT");
            done
        });
    }
    if let Some(joined) = joined {
        let resource = owner
            .return_owned(joined)
            .unwrap_or_else(|_| panic!("wrong operation"));
        assert_eq!(resource.retire(), 2);
    }
    // No normal return/reuse capability is issued on failure or stale IO.
    // This fixture has no native resource to tear down.
}
#[test]
fn normal_return_waits_for_both_actual_completions() {
    scenario(true, true, false);
}
#[test]
fn failed_receive_cannot_mint_normal_return() {
    scenario(false, true, false);
}
#[test]
fn cancelled_transmit_cannot_mint_normal_return() {
    scenario(true, false, false);
}
#[test]
fn previous_operation_completion_cannot_mint_normal_return() {
    scenario(true, true, true);
}

#[test]
fn emergency_stop_does_not_wait_for_a_resident_branch_or_io_join() {
    type Resident = Msg<20, ()>;
    type Stopped = Msg<21, ()>;
    type IoDone = Msg<22, ()>;
    type Reusable = Msg<23, ()>;
    let global = g::par(
        g::send::<0, 1, Resident>().roll(),
        g::par(
            g::send::<2, 3, Stopped>(),
            g::seq(g::send::<4, 3, IoDone>(), g::send::<3, 5, Reusable>()),
        ),
    );
    let rp: RoleProgram<0> = project(&global);
    let cp: RoleProgram<1> = project(&global);
    let sp: RoleProgram<2> = project(&global);
    let op: RoleProgram<3> = project(&global);
    let ip: RoleProgram<4> = project(&global);
    let np: RoleProgram<5> = project(&global);
    let mut slab = [0; 32768];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, TestTransport::new())
        .unwrap();
    let sid = SessionId::new(0x7071);
    let mut resident = rv.enter(sid, &rp).unwrap();
    let mut command = rv.enter(sid, &cp).unwrap();
    let mut stop = rv.enter(sid, &sp).unwrap();
    let mut owner = rv.enter(sid, &op).unwrap();
    let mut io = rv.enter(sid, &ip).unwrap();
    let mut next = rv.enter(sid, &np).unwrap();
    let stopped = Cell::new(false);
    block_on(async {
        // Model the independent native stop before any communication. This
        // assertion is about the integration's ordering, not hardware timing.
        stopped.set(true);
        stop.send::<Stopped>(&()).await.unwrap();
        owner.recv::<Stopped>().await.unwrap();
        assert!(stopped.get());
        {
            let mut waiting = pin!(owner.recv::<IoDone>());
            assert!(once(waiting.as_mut()).is_pending());
            // A resident command still makes progress while finite cleanup waits.
            resident.send::<Resident>(&()).await.unwrap();
            command.recv::<Resident>().await.unwrap();
            io.send::<IoDone>(&()).await.unwrap();
            waiting.await.unwrap();
        }
        owner.send::<Reusable>(&()).await.unwrap();
        next.recv::<Reusable>().await.unwrap();
        // Reuse did not require termination of the resident roll.
        resident.send::<Resident>(&()).await.unwrap();
        command.recv::<Resident>().await.unwrap();
    });
}

#[test]
fn existing_contract_rejects_return_before_actual_receives() {
    let global = choreography();
    let rp: RoleProgram<RX> = project(&global);
    let tp: RoleProgram<TX> = project(&global);
    let op: RoleProgram<OWNER> = project(&global);
    let np: RoleProgram<NEXT> = project(&global);
    let mut slab = [0; 16384];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, TestTransport::new())
        .unwrap();
    let id = SessionId::new(0x7072);
    let mut rx = rv.enter(id, &rp).unwrap();
    let mut tx = rv.enter(id, &tp).unwrap();
    let mut owner = rv.enter(id, &op).unwrap();
    let _next = rv.enter(id, &np).unwrap();
    block_on(async {
        owner.send::<WaitRx>(&7).await.unwrap();
        owner.send::<WaitTx>(&7).await.unwrap();
        rx.recv::<WaitRx>().await.unwrap();
        tx.recv::<WaitTx>().await.unwrap();
        rx.send::<RxDone>(&7).await.unwrap();
        tx.send::<TxDone>(&7).await.unwrap();
        // Carrier acceptance of both notifications does not authorize return:
        // OWNER still has to consume the two actual messages.
        assert!(owner.send::<Released>(&7).await.is_err());
    });
}

static_assertions::assert_not_impl_any!(resource::Owner<'static>: Clone, Copy);
static_assertions::assert_not_impl_any!(resource::Resource<'static>: Clone, Copy);
static_assertions::assert_not_impl_any!(resource::Joined<'static>: Clone, Copy);
