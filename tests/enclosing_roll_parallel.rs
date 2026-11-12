//! A completed enclosing repeat leaves its parallel body available for borrowing.
mod common;
#[path = "../examples/support/in_process.rs"]
pub mod in_process;

use hibana::{
    g,
    runtime::{SessionKitStorage, ids::SessionId, program::project},
};

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn parallel_borrows_do_not_require_restarting_the_enclosing_prefix()
-> core::result::Result<(), hibana::EndpointError> {
    type Prefix = g::Msg<231, ()>;
    type Left = g::Msg<232, ()>;
    type Right = g::Msg<233, ()>;
    let global = g::seq(
        g::send::<0, 1, Prefix>(),
        g::par(g::send::<0, 1, Left>(), g::send::<0, 2, Right>()).roll(),
    )
    .roll();
    let p0 = project::<0>(&global);
    let p1 = project::<1>(&global);
    let p2 = project::<2>(&global);
    let network = common::TestTransport::new();
    let [mut slab0, mut slab1, mut slab2] = [[0; 16 * 1024]; 3];
    let mut storage0 = SessionKitStorage::uninit();
    let mut storage1 = SessionKitStorage::uninit();
    let mut storage2 = SessionKitStorage::uninit();
    let rv0 = storage0
        .init()
        .rendezvous(&mut slab0, network.clone())
        .unwrap();
    let rv1 = storage1
        .init()
        .rendezvous(&mut slab1, network.clone())
        .unwrap();
    let rv2 = storage2
        .init()
        .rendezvous(&mut slab2, network.clone())
        .unwrap();
    let sid = SessionId::new(9284);
    let mut a = rv0.enter(sid, &p0).unwrap();
    let mut b = rv1.enter(sid, &p1).unwrap();
    let mut c = rv2.enter(sid, &p2).unwrap();
    async {
        (async {
            assert!(a.par::<2>().is_err(), "the unfinished prefix is required");
            for _ in 0..2 {
                a.send::<Prefix>(&()).await.unwrap();
                b.recv::<Prefix>().await.unwrap();
                for _ in 0..2 {
                    let [mut left, mut right] = a.par().unwrap();
                    let (sent_left, sent_right, received_left, received_right) = async {
                        async { Ok::<_, hibana::EndpointError>((left.send::<Left>(&())).await) };

                        async { Ok::<_, hibana::EndpointError>((right.send::<Right>(&())).await) };

                        async { Ok::<_, hibana::EndpointError>((b.recv::<Left>()).await) };

                        async { Ok::<_, hibana::EndpointError>((c.recv::<Right>()).await) };
                    }
                    .await
                    .expect("all child results are data");
                    sent_left.unwrap();
                    sent_right.unwrap();
                    received_left.unwrap();
                    received_right.unwrap();
                }
                // Borrow construction and destruction leave the completed
                // visit unchanged; the following Prefix starts the outer one.
                let [left, right] = a.par().unwrap();
                drop((left, right));
            }
        })
        .await;
    }
    .await;
    assert!(network.queue_is_empty());

    Ok(())
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn returned_parallel_loans_can_be_previewed_and_borrowed_in_the_next_round()
-> Result<(), hibana::EndpointError> {
    use hibana::Endpoint;
    type Loan<'r, const ROLE: u8> = g::Msg<1, Endpoint<'r, ROLE>>;
    type Left = g::Msg<2, ()>;
    type Right = g::Msg<3, ()>;
    type Private = g::Msg<4, ()>;
    type Final = g::Msg<5, ()>;
    type LeftAck = g::Msg<6, ()>;
    type RightAck = g::Msg<7, ()>;
    let body = g::seq(
        g::par(g::send::<0, 1, Loan<3>>(), g::send::<0, 1, Loan<4>>()),
        g::par(
            g::seq(
                g::seq(g::send::<3, 2, Left>(), g::send::<2, 1, LeftAck>()).roll(),
                g::send::<1, 0, Loan<3>>(),
            ),
            g::seq(
                g::seq(g::send::<4, 2, Right>(), g::send::<2, 1, RightAck>()).roll(),
                g::send::<1, 0, Loan<4>>(),
            ),
        ),
    )
    .roll();
    let source = g::par(
        g::seq(body, g::send::<0, 1, Final>()),
        g::send::<1, 5, Private>(),
    );
    assert_eq!(g::diagnose(&source), None);
    let p0 = project::<0>(&source);
    let p1 = project::<1>(&source);
    let p2 = project::<2>(&source);
    let p3 = project::<3>(&source);
    let p4 = project::<4>(&source);
    let p5 = project::<5>(&source);
    for parked in [false, true] {
        let network = common::TestTransport::new();
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let kit = storage.init();
        let runtime = kit.rendezvous(&mut slab, network.clone()).unwrap();
        let sid = SessionId::new(9291);
        let mut owner = runtime.enter(sid, &p0).unwrap();
        let mut worker = runtime.enter(sid, &p1).unwrap();
        let mut service = runtime.enter(sid, &p2).unwrap();
        let mut left = runtime.enter(sid, &p3).unwrap();
        let mut right = runtime.enter(sid, &p4).unwrap();
        let mut io = runtime.enter(sid, &p5).unwrap();
        async {
            async {
                for _ in 0..2 {
                    async {
                        let [mut a, mut b] = owner.par()?;
                        async { a.send::<Loan<3>>(left).await };
                        async { b.send::<Loan<4>>(right).await };
                    }
                    .await?;
                    (left, right) = async {
                        let [mut a, mut b] = owner.par()?;
                        async { a.recv::<Loan<3>>().await };
                        async { b.recv::<Loan<4>>().await };
                    }
                    .await?;
                }
                owner.send::<Final>(&()).await?;
                Ok(())
            };
            async {
                let [mut task, mut private] = worker.par()?;
                private.send::<Private>(&()).await?;
                for round in 0..2 {
                    if parked {
                        drop(task.offer().await?);
                    }
                    let (mut left, mut right) = async {
                        let [mut a, mut b] = task.par().unwrap_or_else(|e| {
                            panic!("loan grant round {round}, parked {parked}: {e:?}")
                        });
                        async { a.recv::<Loan<3>>().await };
                        async { b.recv::<Loan<4>>().await };
                    }
                    .await?;
                    async {
                        let [mut a, mut b] = task.par()?;
                        async {
                            left.send::<Left>(&()).await?;
                            a.recv::<LeftAck>().await?;
                            a.send::<Loan<3>>(left).await
                        };
                        async {
                            right.send::<Right>(&()).await?;
                            b.recv::<RightAck>().await?;
                            b.send::<Loan<4>>(right).await
                        };
                    }
                    .await?;
                }
                task.recv::<Final>().await?;
                Ok(())
            };
            async {
                for _ in 0..2 {
                    async {
                        let [mut a, mut b] = service.par()?;
                        async {
                            a.recv::<Left>().await?;
                            a.send::<LeftAck>(&()).await
                        };
                        async {
                            b.recv::<Right>().await?;
                            b.send::<RightAck>(&()).await
                        };
                    }
                    .await?;
                }
                Ok(())
            };
        }
        .await?;
        io.recv::<Private>().await?;
        assert!(network.queue_is_empty());
    }
    Ok(())
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn a_returned_loan_does_not_seal_the_services_retained_inner_repeat()
-> Result<(), hibana::EndpointError> {
    use hibana::{Endpoint, g::Message};
    use std::{future::Future, task::Poll};
    type Loan<'r> = g::Msg<1, Endpoint<'r, 3>>;
    type Formulation = g::Msg<2, ()>;
    type Request = g::Msg<3, ()>;
    type Reply = g::Msg<4, ()>;
    type Final = g::Msg<5, ()>;
    let body = g::seq(
        g::send::<0, 1, Loan>(),
        g::seq(
            g::send::<0, 2, Formulation>(),
            g::seq(
                g::seq(g::send::<3, 2, Request>(), g::send::<2, 1, Reply>()).roll(),
                g::send::<1, 0, Loan>(),
            ),
        ),
    )
    .roll();
    let source = g::seq(body, g::send::<0, 2, Final>());
    assert_eq!(g::diagnose(&source), None);
    let p0 = project::<0>(&source);
    let p1 = project::<1>(&source);
    let p2 = project::<2>(&source);
    let p3 = project::<3>(&source);
    let network = common::TestTransport::new();
    let mut slab = [0; 64 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let runtime = storage
        .init()
        .rendezvous(&mut slab, network.clone())
        .unwrap();
    let sid = SessionId::new(9292);
    let mut owner = runtime.enter(sid, &p0).unwrap();
    let mut worker = runtime.enter(sid, &p1).unwrap();
    let mut service = runtime.enter(sid, &p2).unwrap();
    let material = runtime.enter(sid, &p3).unwrap();

    owner.send::<Loan>(material).await?;
    let mut material = worker.recv::<Loan>().await?;
    owner.send::<Formulation>(&()).await?;
    service.recv::<Formulation>().await?;
    material.send::<Request>(&()).await?;
    service.recv::<Request>().await?;
    service.send::<Reply>(&()).await?;
    worker.recv::<Reply>().await?;
    worker.send::<Loan>(material).await?;
    let material = owner.recv::<Loan>().await?;
    owner.send::<Loan>(material).await?;
    let mut material = worker.recv::<Loan>().await?;
    material.send::<Request>(&()).await?;

    // The old inner repeat remains an available preview. Returning and
    // regaining the same affine value does not turn an elastic roll into a
    // closed loop. A dropped offer does not choose which visit will consume
    // the request: the explicit formulation receive can enter the next one.
    {
        let mut offered = core::pin::pin!(service.offer());
        core::future::poll_fn(|cx| match offered.as_mut().poll(cx) {
            Poll::Pending => panic!("the retained inner repeat has an incoming request"),
            Poll::Ready(result) => {
                let branch = result.expect("retained inner repeat preview");
                assert_eq!(branch.label(), Request::LOGICAL_LABEL);
                drop(branch);
                Poll::Ready(())
            }
        })
        .await;
    }
    owner.send::<Formulation>(&()).await?;
    service.recv::<Formulation>().await?;
    service.recv::<Request>().await?;
    service.send::<Reply>(&()).await?;
    worker.recv::<Reply>().await?;
    worker.send::<Loan>(material).await?;
    let _material = owner.recv::<Loan>().await?;
    owner.send::<Final>(&()).await?;
    service.recv::<Final>().await?;
    assert!(network.queue_is_empty());
    Ok(())
}
